//! Devastator as the walking giant (`tf2_core::giant`), run headless on the user's install: he
//! walks, turns on the spot, pounds (whose shockwaves hurt what they pass) and starts the vortex,
//! and each of those ends back where it started. Skipped when the install is not found (`tf2_core::install`).

use std::path::Path;

use glam::{Vec2, Vec3};
use tf2_core::character::{self, CharacterData};
use tf2_core::control::button;
use tf2_core::melee::Target;
use tf2_core::sim::{self, Arena, Input, State};
use tf2_core::tuning::{self, Actions, Locomotion, RuleSet, Tuning};

const DT: f32 = 0.032;

/// His tuning as `game\src\charmenu.rs` builds it, `giant::fit` last.
fn devastator() -> Option<(Tuning, CharacterData)> {
    let dir = tf2_core::install::dir();
    if !dir.exists() {
        return None;
    }
    let mut t = tuning::load(dir, "Devastator").expect("Devastator's attributes");
    let mut data = character::load(dir, "devastator").expect("Devastator's pack");
    tf2_core::giant::reroot(&mut data.animations, &data.robot);
    t.locomotion = Locomotion::from_animations(&data.animations, &mut t.missing);
    t.actions = Actions::from_animations(&data.animations, &data.robot, &mut t.missing);
    t.weapon_set = RuleSet::from_animations(&data.animations, "WeaponSet", &mut t.missing);
    t.rule_sets = RuleSet::all(&data.animations);
    if let Some(camera) = t.robot_camera_levels.first() {
        t.robot_camera = camera.clone();
    }
    tf2_core::giant::fit(&mut t, &data.animations, &data.robot, &data.shockwaves);
    let vortex = tf2_core::giant::read_vortex(dir).and_then(|def| tf2_core::giant::vortex(&def, &data.library, &data.animations, &data.robot));
    t.giant.as_mut().unwrap().vortex = vortex;
    Some((t, data))
}

fn press(b: u32) -> Input {
    Input { buttons_down: 1 << b, buttons_hit: 1 << b, ..Input::default() }
}

/// Runs until `until` holds or `seconds` pass; the seconds it took.
fn run_until(s: &mut State, t: &Tuning, arena: &Arena, input: Input, seconds: f32, until: impl Fn(&State) -> bool) -> Option<f32> {
    for i in 0..(seconds / DT).round() as usize {
        sim::step(s, &input, t, arena, DT);
        if until(s) {
            return Some((i + 1) as f32 * DT);
        }
    }
    None
}

fn standing(t: &Tuning) -> State {
    let mut s = State::new(t);
    run_until(&mut s, t, &Arena::default(), Input::default(), 0.5, |_| false);
    s
}

#[test]
fn his_binding_is_read_from_his_machine() {
    let Some((t, data)) = devastator() else { return };
    let giant = t.giant.as_ref().expect("fit sets the binding");
    // The four messages his brain sends. [data global_states.txt 6484]
    assert_eq!(giant.attack, Some(2874311729));
    assert_eq!(giant.special, Some(3458736789));
    assert!(giant.turn_left.is_some() && giant.turn_right.is_some());
    // His object's two shockwaves, both named by the pound's Trigger events. [data]
    assert_eq!(data.shockwaves.len(), 2);
    for wave in &giant.waves {
        // [data] 25, raised for a player (`POUND_DAMAGE`).
        assert_eq!(wave.def.damage, tf2_core::giant::POUND_DAMAGE);
        assert!(wave.centres.contains_key(&tf2_core::formats::hash::crc32(b"Attack1")), "{:?}", wave.def.name);
    }
    // Re-rooted on his chest, no clip jumps him: the windup's 35.2 m root jump is gone. The
    // biggest step left is the pound's lurch as the first blow lands (`Attack1` tick 96, 1.6 m).
    for (name, clip) in t.actions.clips.iter() {
        let jump = clip.path.windows(2).map(|w| (w[1] - w[0]).length()).fold(0.0, f32::max);
        assert!(jump < 2.5, "{name:08x} steps {jump} m in a tick");
    }
    assert!(t.actions.named("Idle2Attack1").is_some());
    assert!(t.actions.plays(tf2_core::formats::hash::crc32(b"TurnSet")));
}

/// Re-rooting moves no node of his: with the root channel applied, every node is where the clip
/// as read puts it, at every clip and tick; and with the root held (as drawn) his chest stays
/// over the origin.
#[test]
fn rerooting_keeps_him_where_his_clips_put_him() {
    use glam::{Affine3A, Quat};
    use tf2_core::formats::anim::{Clip, ROOT_CHANNEL};
    let dir = tf2_core::install::dir();
    if !dir.exists() {
        return;
    }
    let read = character::load(dir, "devastator").expect("Devastator's pack");
    let mut moved = character::load(dir, "devastator").expect("Devastator's pack");
    tf2_core::giant::reroot(&mut moved.animations, &moved.robot);
    let rig = tf2_core::pose::Rig::new(&read.robot);
    let centre = read.robot.nodes.iter().position(|n| n.name == tf2_core::giant::CENTRE).unwrap();
    // The root channel's transform at a tick (stored for row vectors: conjugated).
    let root_at = |clip: &Clip, tick: f32| {
        let c = clip.channel(ROOT_CHANNEL);
        let p = c.and_then(|c| c.position.sample(tick, clip.length, false)).map_or(Vec3::ZERO, Vec3::from);
        let q = c.and_then(|c| c.rotation.sample(tick, clip.length, false)).map_or(Quat::IDENTITY, |q| Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize().conjugate());
        Affine3A::from_rotation_translation(q, p)
    };
    let (mut clips, mut worst, mut drift) = (0, 0.0f32, 0.0f32);
    for (set, refs) in &read.animations.sets {
        for r in refs.iter().filter(|r| r.clip.is_some()) {
            let (a, b) = (r.clip.as_ref().unwrap(), moved.animations.find(set, &r.id).and_then(|m| m.clip.as_ref()).unwrap());
            clips += 1;
            let mut tick = 0.0;
            while tick <= a.length {
                let (pa, pb) = (rig.pose(a, r.looping, tick), rig.pose(b, r.looping, tick));
                let (ra, rb) = (root_at(a, tick), root_at(b, tick));
                for (na, nb) in pa.iter().zip(&pb).skip(1) {
                    worst = worst.max((ra.transform_point3a(na.translation) - rb.transform_point3a(nb.translation)).length());
                }
                drift = drift.max(pb[centre].translation.truncate().length());
                tick += 7.0;
            }
        }
    }
    println!("{clips} clips: nodes moved at most {worst} m, chest off the origin at most {drift} m");
    assert!(clips >= 15);
    assert!(worst < 0.01, "{worst}");
    assert!(drift < 0.01, "{drift}");
}

#[test]
fn he_walks_forward_and_stops() {
    let Some((t, _)) = devastator() else { return };
    let arena = Arena::default();
    let mut s = standing(&t);
    let walk = Input { stick: Vec2::Y, ..Input::default() };
    run_until(&mut s, &t, &arena, walk, 6.0, |_| false);
    assert_eq!(s.state_name(&t), Some("stateMove"));
    println!("walked to {:?}, yaw {:.3}", s.pos, s.yaw);
    // He goes by his chest, which sways as he walks: 2 m to the right over `IdleToWalk`, then
    // from 5.3 m left to 2 m right of its line over each `Walk` loop. [data `devastator_clips
    // --reroot --slide`]
    assert!(s.pos.y > 10.0 && s.pos.x.abs() < 6.0, "{:?}", s.pos);
    let stopped = run_until(&mut s, &t, &arena, Input::default(), 6.0, |s| s.state_name(&t) == Some("stateIdle"));
    assert!(stopped.is_some(), "still {:?}", s.state_name(&t));
}

/// Each hand and foot he lands on thuds (a shake at his place and the landing rumble) at the
/// walk's own footfall ticks, and when he lets go of the stick he runs down into his stand over
/// `giant::IDLE_BLEND` instead of stopping in one step. [assumed port design]
#[test]
fn his_steps_thud_and_he_runs_down_into_his_stand() {
    let Some((t, _)) = devastator() else { return };
    let arena = Arena::default();
    let mut s = standing(&t);
    let walk = Input { stick: Vec2::Y, ..Input::default() };
    let mut thuds = Vec::new();
    for i in 0..(8.0 / DT) as usize {
        sim::step(&mut s, &walk, &t, &arena, DT);
        if s.shakes.iter().any(|k| k.name == tf2_core::giant::FOOTFALL_SHAKE) {
            assert!(!s.rumbles.is_empty(), "a thud without its rumble at step {i}");
            thuds.push((i as f32 * DT, s.base_clip(&t).map(|b| (b.id, b.tick))));
        }
    }
    println!("thuds {thuds:?}");
    // Two loops of the walk's four landings (it is 4 s long), give or take the start.
    assert!((6..=10).contains(&thuds.len()), "{thuds:?}");
    let mut speeds = Vec::new();
    let mut last = s.pos;
    for _ in 0..(1.5 / DT) as usize {
        sim::step(&mut s, &Input::default(), &t, &arena, DT);
        speeds.push((s.pos - last).truncate().length() / DT);
        last = s.pos;
    }
    println!("speeds after letting go {:?}", speeds.iter().map(|v| (v * 10.0).round() / 10.0).collect::<Vec<_>>());
    // Still moving, and slowing, a third of a second on; in his stand by the blend's end, where
    // only his Idle's own sway (his chest, which he goes by, rocks over his planted limbs at up
    // to 4 m/s [data]) moves him.
    let third = (0.3 / DT) as usize;
    assert!(speeds[third] > 2.0 && speeds[third] < speeds[1] && speeds[1] < 17.0, "{speeds:?}");
    assert_eq!(s.base_clip(&t).map(|b| b.id), Some("Idle"));
    assert!(speeds[(tf2_core::giant::IDLE_BLEND / DT) as usize..].iter().all(|v| *v < 4.5), "{speeds:?}");
}

/// His walk moves him by his clips' root (`giant::Giant::roots`): drawn as the game draws him (the
/// base clip at the sim's tick, the root held, placed at his place and heading), some limb of his
/// stays put on the ground at every step, from the start of the walk on. At the walk's even speed
/// his planted hands slipped up to 0.23 m a tick (`devastator_clips --reroot --slide Walk`).
#[test]
fn his_walk_pushes_off_his_planted_limbs() {
    use glam::{Affine3A, Quat};
    let Some((t, data)) = devastator() else { return };
    let arena = Arena::default();
    let mut s = standing(&t);
    let rig = tf2_core::pose::Rig::new(&data.robot);
    let limbs: Vec<usize> = ["Foot_L", "Foot_R", "F_Middle_Tip_L", "F_Middle_Tip_R"]
        .iter()
        .map(|n| data.robot.nodes.iter().position(|x| x.name == *n).unwrap())
        .collect();
    let drawn = |s: &State| -> Option<Vec<Vec3>> {
        let base = s.base_clip(&t)?;
        let r = data.animations.find(base.set, base.id)?;
        let pose = rig.pose(r.clip.as_ref()?, r.looping, base.tick);
        let at = Affine3A::from_rotation_translation(Quat::from_rotation_z(s.yaw), s.pos);
        Some(limbs.iter().map(|&n| at.transform_point3(pose[n].translation.into())).collect())
    };
    let walk = Input { stick: Vec2::Y, ..Input::default() };
    let (start, mut last, mut steps, mut total, mut worst) = (s.pos, drawn(&s), 0, 0.0f32, 0.0f32);
    let mut since = 0.0;
    for _ in 0..(8.0 / DT) as usize {
        let serial = s.base_clip(&t).map(|b| b.serial);
        sim::step(&mut s, &walk, &t, &arena, DT);
        let now = drawn(&s);
        let base = s.base_clip(&t);
        since = if serial == base.map(|b| b.serial) { since + DT } else { 0.0 };
        // A change of clip is a cross-fade in the drawing, not a step of one clip: the clip
        // before goes on moving him through it too (`sim::giant_root_motion`).
        if let (Some(a), Some(b)) = (&last, &now) && since > base.map_or(0.0, |b| b.blend) {
            let still = a.iter().zip(b).map(|(a, b)| (*b - *a).truncate().length()).fold(f32::MAX, f32::min);
            (steps, total, worst) = (steps + 1, total + still, worst.max(still));
        }
        last = now;
    }
    let speed = (s.pos - start).length() / 8.0;
    let mean = total / steps as f32;
    println!("{steps} steps: the stillest limb moved {mean:.3} m a step on average, at most {worst:.3}; {speed:.2} m/s");
    assert_eq!(s.state_name(&t), Some("stateMove"));
    assert!(steps > 200, "{steps}");
    assert!(mean < 0.02 && worst < 0.08, "{mean} {worst}");
    // The walk's own pace: 43.85 m in its 4 s loop, after the 15.6 m of `IdleToWalk`. [data]
    assert!((9.0..13.0).contains(&speed), "{speed}");
}

#[test]
fn a_push_to_his_left_turns_him_on_the_spot() {
    let Some((t, _)) = devastator() else { return };
    let arena = Arena::default();
    let mut s = standing(&t);
    let left = Input { stick: Vec2::NEG_X, ..Input::default() };
    let entered = run_until(&mut s, &t, &arena, left, 0.2, |s| s.state_name(&t) == Some("stateTurnLeft"));
    assert!(entered.is_some(), "in {:?}", s.state_name(&t));
    let start = s.pos;
    // The clip runs out (240 ticks, 4 s) and the turn leaves into stateMove, then stateIdle.
    let back = run_until(&mut s, &t, &arena, Input::default(), 8.0, |s| s.state_name(&t) == Some("stateIdle"));
    println!("turned to yaw {:.1} deg, moved {:?}", s.yaw.to_degrees(), s.pos - start);
    assert!(back.is_some(), "still {:?}", s.state_name(&t));
    assert!((s.yaw.to_degrees() - 90.0).abs() < 2.0, "yaw {}", s.yaw.to_degrees());
    // On the spot: his chest ends a few metres from where it was (the root crosses 22.5 m of his
    // body, `giant::CENTRE`).
    assert!((s.pos - start).length() < 6.0, "moved {:?}", s.pos - start);
}

#[test]
fn the_pound_sends_its_shockwaves_and_ends_in_idle() {
    let Some((t, _)) = devastator() else { return };
    // A scout standing 40 m ahead of him: the rings reach it when their radius passes 40.
    let scout = Target {
        id: 7,
        pos: Vec3::new(0.0, 40.0, 0.0),
        radius: 0.6,
        height: 2.0,
        character: true,
        large: false,
        surface: 0,
        dashing: false,
        health: 100.0,
        head: None,
    };
    let arena = Arena { targets: vec![scout], ..Arena::default() };
    let mut s = standing(&t);
    sim::step(&mut s, &press(button::ATTACK_FAST), &t, &arena, DT);
    run_until(&mut s, &t, &arena, Input::default(), 0.2, |_| false);
    assert_eq!(s.state_name(&t), Some("stateAttackDevastator"));
    let start = s.pos;
    let (mut rings, mut hits) = (0, Vec::new());
    let done = run_until(&mut s, &t, &arena, Input::default(), 12.0, |s| {
        s.state_name(&t) == Some("stateIdle")
    });
    // Count again over a fresh pound, watching every step.
    assert!(done.is_some(), "the pound never ended: {:?}", s.state_name(&t));
    println!("pound took {:.2} s, moved {:?}", done.unwrap(), s.pos - start);
    // The windup starts in his walking frame and rears him back onto his hind legs: from
    // standing, his chest ends about 14 m back (`giant::reroot`), never by a jump.
    assert!((s.pos - start).length() < 16.0 && s.pos.y < start.y, "the windup moved him {:?}", s.pos - start);
    sim::step(&mut s, &press(button::ATTACK_FAST), &t, &arena, DT);
    for _ in 0..(12.0 / DT) as usize {
        sim::step(&mut s, &Input::default(), &t, &arena, DT);
        rings = rings.max(s.shockwaves.iter().map(|w| w.rings.len()).sum::<usize>());
        hits.extend(s.hits.iter().filter(|h| h.target == 7).map(|h| (h.damage, h.stun_time)));
        if s.state_name(&t) == Some("stateIdle") {
            break;
        }
    }
    println!("most rings at once {rings}, hits on the scout {hits:?}");
    assert!(rings >= 1);
    // Each Trigger starts a ring, and each wave hurts a thing once while any of its rings lives.
    assert!(!hits.is_empty() && hits.iter().all(|&(damage, stun)| damage == tf2_core::giant::POUND_DAMAGE && stun == 1.0), "{hits:?}");
}

#[test]
fn the_special_plays_the_vortex_and_ends() {
    let Some((t, _)) = devastator() else { return };
    let arena = Arena::default();
    let mut s = standing(&t);
    sim::step(&mut s, &press(button::ATTACK_SPECIAL), &t, &arena, DT);
    run_until(&mut s, &t, &arena, Input::default(), 0.2, |_| false);
    assert_eq!(s.state_name(&t), Some("stateAttackSpecialDevastator"));
    let start = s.pos;
    // Idle2AttackSpecial (56 ticks) then AttackSpecial (726): about 13 s.
    let done = run_until(&mut s, &t, &arena, Input::default(), 20.0, |s| s.state_name(&t) == Some("stateIdle"));
    println!("special took {:?} s, moved {:?}", done, s.pos - start);
    assert!(done.is_some_and(|d| d > 12.0), "{done:?} in {:?}", s.state_name(&t));
    // The same windup as the pound's (`Idle2AttackSpecial`).
    assert!((s.pos - start).length() < 16.0, "{:?}", s.pos - start);
}

/// The mission's vortex read and put on his head: the funnel's own clip grows it to 50 times
/// (101 m), its KillBox hits for 50, and through `AttackSpecial` it drags a scout standing in front
/// of him toward his head and keeps hitting it, no more often than every half second.
#[test]
fn his_vortex_drags_in_and_hits_what_is_in_front_of_him() {
    let Some((t, _)) = devastator() else { return };
    let vortex = t.giant.as_ref().and_then(|g| g.vortex.as_ref()).expect("the mission's VortexObj");
    println!(
        "vortex: starts at tick {}, {} ticks, scale up to {}, length {:.2}, profile {:?}, damage {} flags {:x}, knock-back {} at {}, sound {:08x} after {} s",
        vortex.start, vortex.scale.len(), vortex.scale.iter().copied().fold(0.0, f32::max), vortex.length, vortex.profile,
        vortex.damage, vortex.flags, vortex.knock_back_speed, vortex.knock_back_angle, vortex.sound, vortex.sound_delay
    );
    assert_eq!(vortex.start, 86.0);
    assert_eq!(vortex.scale.len(), 521);
    assert!((vortex.scale.iter().copied().fold(0.0, f32::max) - 50.0).abs() < 0.1);
    assert!((vortex.length - 2.02).abs() < 0.01);
    // [data] damage 50, raised for a player (`VORTEX_DAMAGE`).
    assert_eq!((vortex.damage, vortex.knock_back_speed, vortex.knock_back_angle), (tf2_core::giant::VORTEX_DAMAGE, 15.0, 35.0));
    assert_eq!((vortex.sound, vortex.sound_delay), (1788064107, 5.0));
    // The funnel runs level from his head (14 m up through the special) the way it looks, and he
    // rears back about 30 m through the windup: a scout 40 m ahead of where he stood is 46 m along
    // it and 13 m under its axis, where it is 25 m across.
    let mut s = standing(&t);
    let scout = Target {
        id: 9,
        pos: s.pos + Vec3::new(0.0, 40.0, 0.0),
        radius: 0.6,
        height: 2.0,
        character: true,
        large: false,
        surface: 0,
        dashing: false,
        health: 100.0,
        head: None,
    };
    let mut arena = Arena { targets: vec![scout], ..Arena::default() };
    sim::step(&mut s, &press(button::ATTACK_SPECIAL), &t, &arena, DT);
    let (mut up, mut pulled, mut hits, mut time) = (0.0, Vec::new(), Vec::new(), 0.0);
    for _ in 0..(20.0 / DT) as usize {
        sim::step(&mut s, &Input::default(), &t, &arena, DT);
        time += DT;
        if s.vortex.is_some() {
            up += DT;
        }
        for &(_, v) in s.pulls.iter().filter(|p| p.0 == 9) {
            // The host moves the scout by its drag, toward the funnel's point (on his head).
            let at = &mut arena.targets[0].pos;
            let tip = s.vortex.map_or(s.pos, |f| f.tip);
            pulled.push(((tip - (*at + Vec3::Z * 0.6)).normalize_or_zero().dot(v.normalize_or_zero()), v.length()));
            *at += v * DT;
        }
        hits.extend(s.hits.iter().filter(|h| h.target == 9).map(|h| (time, h.damage)));
        if s.state_name(&t) == Some("stateIdle") {
            break;
        }
    }
    let least = pulled.iter().filter(|p| p.1 > 0.5).map(|p| p.0).fold(f32::MAX, f32::min);
    println!("funnel up {up:.2} s, dragged {} steps (toward its point at least {least:.3}), hits {hits:?}, scout ends {:?} from him", pulled.len(), arena.targets[0].pos - s.pos);
    // Its own clip, 520 ticks.
    assert!((up - 520.0 / 60.0).abs() < 0.1, "{up}");
    // Dragged in to his head and held there (where the drag is all but nothing), hit all the while.
    assert!(pulled.len() > 200 && least > 0.5, "{pulled:?}");
    assert!(hits.len() >= 3 && hits.iter().all(|&(_, d)| d == tf2_core::giant::VORTEX_DAMAGE), "{hits:?}");
    assert!(hits.windows(2).all(|w| w[1].0 - w[0].0 >= 0.5 - 1e-3), "{hits:?}");
}

#[test]
fn he_cannot_jump() {
    let Some((t, _)) = devastator() else { return };
    let mut s = standing(&t);
    let jump = Input { jump: true, jump_held: true, ..Input::default() };
    run_until(&mut s, &t, &Arena::default(), jump, 0.5, |_| false);
    assert_eq!(s.state_name(&t), Some("stateIdle"));
    assert!(s.pos.z.abs() < 0.01);
}

/// The port's planting of his arms and legs (`giant::limbs`): four limbs, each sole where it stands
/// in `Idle`; on flat ground they stay as the clip has them, and a block under one hand lifts that
/// hand onto it without stretching the arm.
#[test]
fn his_arms_and_legs_are_planted_on_the_ground() {
    use glam::Affine3A;
    use tf2_core::footik::{accumulate, FootIk};
    use tf2_core::terrain::Ramp;
    let Some((_, data)) = devastator() else { return };
    let rig = tf2_core::giant::limbs(&data.animations, &data.robot, &data.library).expect("his limbs");
    let nodes = &data.robot.nodes;
    let names: Vec<_> = rig.legs.iter().map(|l| [&nodes[l.hip].name, &nodes[l.knee].name, &nodes[l.foot].name]).collect();
    println!("limbs {names:?}, probe {}", rig.probe);
    assert_eq!(names.iter().map(|n| n[2].as_str()).collect::<Vec<_>>(), ["Foot_L", "Foot_R", "Hand_L", "Hand_R"]);
    assert_eq!((names[2][0].as_str(), names[2][1].as_str()), ("Arm_L", "Forearm_L"));
    assert!(rig.probe > 10.0, "{}", rig.probe);
    let idle = data.animations.find("IdleStandSet", "Idle").expect("Idle");
    let world = tf2_core::pose::Rig::new(&data.robot).pose(idle.clip.as_ref().unwrap(), idle.looping, 0.0);
    let parents: Vec<_> = nodes.iter().map(|n| n.parent).collect();
    let base: Vec<_> = world.iter().enumerate().map(|(i, &w)| parents[i].map_or(w, |p| world[p].inverse() * w)).collect();
    // Every sole is on the ground he stands on in `Idle`.
    for leg in &rig.legs {
        for corner in leg.sole {
            let z = world[leg.foot].transform_point3(corner).z;
            assert!(z.abs() < 1.0, "{} sole at {z}", nodes[leg.foot].name);
        }
    }
    let settle = |arena: &Arena| {
        let mut ik = FootIk::default();
        let mut pose = base.clone();
        for _ in 0..50 {
            pose.clone_from_slice(&base);
            ik.apply(&rig, &mut pose, &parents, Affine3A::IDENTITY, true, DT, arena);
        }
        accumulate(&pose, &parents)
    };
    let lengths = |w: &[Affine3A]| -> Vec<f32> {
        rig.legs.iter().flat_map(|l| [(l.hip, l.knee), (l.knee, l.foot)]).map(|(a, b)| Vec3::from(w[a].translation).distance(w[b].translation.into())).collect()
    };
    let rise = |after: &[Affine3A]| -> Vec<f32> { rig.legs.iter().map(|l| after[l.foot].translation.z - world[l.foot].translation.z).collect() };
    let flat = settle(&Arena::default());
    println!("flat: rise {:?}", rise(&flat));
    assert!(rise(&flat).iter().all(|r| r.abs() < 0.05));
    // A block 2 m high under his right hand alone.
    let hand = &rig.legs[3];
    let corners = hand.sole.map(|c| world[hand.foot].transform_point3(c).truncate());
    let min = corners.iter().fold(Vec2::splat(f32::INFINITY), |a, &c| a.min(c)) - Vec2::splat(0.5);
    let max = corners.iter().fold(Vec2::splat(f32::NEG_INFINITY), |a, &c| a.max(c)) + Vec2::splat(0.5);
    let block = Arena { ramps: vec![Ramp { min, max, base: 0.0, height: 2.0, slope: Vec2::ZERO }], ..Arena::default() };
    let lifted = settle(&block);
    let r = rise(&lifted);
    println!("block under Hand_R: rise {r:?}");
    assert!((r[3] - 2.0).abs() < 0.1, "{r:?}");
    assert!(r[..3].iter().all(|r| r.abs() < 0.05), "{r:?}");
    for (a, b) in lengths(&world).iter().zip(lengths(&lifted)) {
        assert!((a - b).abs() < 1e-3, "{a} {b}");
    }
}

/// His limbs plant with weight (`footik::Weight`, `giant::WEIGHT`) [assumed port design]: a hand
/// rising onto a 2 m block takes about half a second (the game's filter took 0.1), his body rises a
/// share of it with a bob and settles, and a hand moving at a block starts rising before it is over it.
#[test]
fn his_limbs_climb_onto_a_block_with_weight() {
    use glam::Affine3A;
    use tf2_core::footik::{accumulate, FootIk};
    use tf2_core::terrain::Ramp;
    let Some((_, data)) = devastator() else { return };
    let rig = tf2_core::giant::limbs(&data.animations, &data.robot, &data.library).expect("his limbs");
    let weight = rig.weight.expect("he is heavy");
    let nodes = &data.robot.nodes;
    let idle = data.animations.find("IdleStandSet", "Idle").expect("Idle");
    let world = tf2_core::pose::Rig::new(&data.robot).pose(idle.clip.as_ref().unwrap(), idle.looping, 0.0);
    let parents: Vec<_> = nodes.iter().map(|n| n.parent).collect();
    let base: Vec<_> = world.iter().enumerate().map(|(i, &w)| parents[i].map_or(w, |p| world[p].inverse() * w)).collect();
    let hand = &rig.legs[3];
    let corners = hand.sole.map(|c| world[hand.foot].transform_point3(c).truncate());
    let min = corners.iter().fold(Vec2::splat(f32::INFINITY), |a, &c| a.min(c)) - Vec2::splat(0.5);
    let max = corners.iter().fold(Vec2::splat(f32::NEG_INFINITY), |a, &c| a.max(c)) + Vec2::splat(0.5);
    let block = Arena { ramps: vec![Ramp { min, max, base: 0.0, height: 2.0, slope: Vec2::ZERO }], ..Arena::default() };
    // Standing: the block appears under his right hand.
    let mut ik = FootIk::default();
    let mut pose = base.clone();
    let (mut at_tenth, mut body_top) = (0.0, f32::MIN);
    for frame in 1..=(2.0 / DT) as usize {
        pose.clone_from_slice(&base);
        ik.apply(&rig, &mut pose, &parents, Affine3A::IDENTITY, true, DT, &block);
        if (frame as f32 * DT - 0.1).abs() < DT * 0.5 {
            at_tenth = ik.lifts()[3];
        }
        body_top = body_top.max(ik.body_lift());
    }
    let after = accumulate(&pose, &parents);
    let rise = after[hand.foot].translation.z - world[hand.foot].translation.z;
    println!("hand at 0.1 s {at_tenth:.2}, at 2 s {rise:.2}; body top {body_top:.2}, at 2 s {:.2}", ik.body_lift());
    assert!(at_tenth < 1.0, "rises slowly: {at_tenth} m in 0.1 s");
    assert!((rise - 2.0).abs() < 0.1, "lands on it: {rise}");
    let held = weight.heave * 2.0;
    assert!(body_top > held + 0.02, "his body bobs over its rest: top {body_top}, rest {held}");
    assert!((ik.body_lift() - held).abs() < 0.05, "and settles: {}", ik.body_lift());
    // Walking at the block (the whole drawn body moving 8 m/s toward it, from 6 m short): the hand
    // starts up before its sole's leading edge is over the block.
    let mut ik = FootIk::default();
    let lead_edge = |shift: f32| corners.iter().map(|c| c.x).fold(f32::MIN, f32::max) + shift;
    let start = min.x - lead_edge(0.0) - 6.0;
    let mut early = None;
    for frame in 0..(2.0 / DT) as usize {
        let shift = start + 8.0 * frame as f32 * DT;
        pose.clone_from_slice(&base);
        ik.apply(&rig, &mut pose, &parents, Affine3A::from_translation(Vec3::X * shift), true, DT, &block);
        if ik.lifts()[3] > 0.2 {
            early = Some(min.x - lead_edge(shift));
            break;
        }
    }
    let early = early.expect("the hand rose");
    println!("hand 0.2 m up {early:.2} m short of the block");
    assert!(early > 0.0, "rises before it is over the block: {early}");
}

/// An Autobot scout's rounds meet his body where his model is: on his cylinder's side, above his
/// ankles, not at the 1 m post under his chest his attributes give him.
#[test]
fn an_autobot_scouts_rounds_hit_his_body() {
    let Some((t, _)) = devastator() else { return };
    // The game's aim point on him is kept: 35 m up [data: aimingHeightPercentage 3500 of 1 m].
    assert!((t.robot_height * t.aiming_height_percentage * 0.01 - 35.0).abs() < 0.01);
    let scout = tf2_core::combat::Scout::load_as(tf2_core::install::dir(), "aprotscoutmp", "APrtScout").expect("the Autobot scout");
    let mut s = standing(&t);
    let mut enemy = tf2_core::combat::Enemy::new(1, Vec3::new(0.0, 70.0, 0.0), &scout);
    let mut landed = Vec::new();
    for _ in 0..(12.0 / DT) as usize {
        for shot in enemy.step(&scout, &s, &t, &Arena::default(), false, DT) {
            if shot.target == Some(s.actor.id) {
                landed.push(shot.to);
            }
        }
        sim::step(&mut s, &Input::default(), &t, &Arena::default(), DT);
    }
    let on_body = |p: &Vec3| ((p - s.pos).truncate().length() - t.robot_radius).abs() < 1.5 && p.z > 5.0;
    println!("{} rounds hit him, at {:?}", landed.len(), landed.iter().take(4).collect::<Vec<_>>());
    assert!(!landed.is_empty(), "no round hit him");
    assert!(landed.iter().all(on_body), "a round hit off his body: {landed:?}");
}

/// Played, he can be hurt (`giant::fit`): enemy hits take his health, small ones leave him be,
/// a burst staggers him into the hit react for the side it came from, which hands him back to
/// his stand; and with no health left he falls in `DeathSet/Death`.
#[test]
fn hits_stagger_him_into_his_hit_reacts_and_kill_him() {
    let Some((t, _)) = devastator() else { return };
    assert!(!t.damage.invincible && t.damage.immovable);
    assert_eq!(t.base_health, tf2_core::giant::GIANT_HEALTH);
    let enemy = tf2_core::damage::Actor { id: 99, team: t.damage.team.wrapping_add(1), ..Default::default() };
    let hit = |amount: f32, direction: Vec3| tf2_core::damage::Packet { source: Some(enemy), amount, direction, ..Default::default() };
    let mut s = standing(&t);
    // A scout's round: health goes (less his armour), no stagger. Bursts are sized through the armour.
    s.receive_damage(hit(10.0, Vec3::Y), &t);
    sim::step(&mut s, &Input::default(), &t, &Arena::default(), DT);
    assert_eq!(s.health, t.base_health - 10.0 * tf2_core::giant::GIANT_ARMOUR);
    assert_eq!(s.giant_staggers, 0);
    // A burst from his front (pushing him back): the reel back.
    let ahead = Vec3::new(-s.yaw.sin(), s.yaw.cos(), 0.0);
    s.receive_damage(hit(t.base_health * 0.03 / tf2_core::giant::GIANT_ARMOUR, -ahead), &t);
    sim::step(&mut s, &Input::default(), &t, &Arena::default(), DT);
    assert_eq!(s.giant_staggers, 1);
    assert_eq!(s.action_clip(&t).map(|(set, id, _)| (set, id)), Some(("PlayAnimSet", "HitReactMissilePod")));
    // The clip runs out and he is back on his feet.
    let back = run_until(&mut s, &t, &Arena::default(), Input::default(), 8.0, |s| s.action.is_none() && matches!(s.state_name(&t), Some("stateIdle" | "stateMove")));
    println!("back in {back:?} s");
    assert!(back.is_some(), "he never left his hit react: {:?}", s.state_name(&t));
    // From his left (pushing him right): the left arm's.
    let right = Vec3::new(s.yaw.cos(), s.yaw.sin(), 0.0);
    run_until(&mut s, &t, &Arena::default(), Input::default(), 3.0, |_| false);
    s.receive_damage(hit(t.base_health * 0.03 / tf2_core::giant::GIANT_ARMOUR, right), &t);
    sim::step(&mut s, &Input::default(), &t, &Arena::default(), DT);
    assert_eq!(s.action_clip(&t).map(|(_, id, _)| id), Some("HitReactArmLt"));
    // The rest of his health: he falls.
    s.receive_damage(hit(t.base_health / tf2_core::giant::GIANT_ARMOUR, -ahead), &t);
    run_until(&mut s, &t, &Arena::default(), Input::default(), 1.0, |_| false);
    assert!(s.dead());
    assert_eq!(s.action_clip(&t).map(|(set, id, _)| (set, id)), Some(("DeathSet", "Death")));
}

/// His body is his model's size (`giant::body`): a wall taller than his ankles stops him a body's
/// radius short, and a lower one passes between his legs while he stays on the ground.
#[test]
fn walls_stop_his_body_and_low_ones_pass_under_his_stride() {
    let Some((t, _)) = devastator() else { return };
    let stride = t.giant.as_ref().unwrap().stride;
    println!("body: radius {:.1}, height {:.1}, stride {stride:.1}", t.robot_radius, t.robot_height);
    assert!((20.0..30.0).contains(&t.robot_radius), "radius {}", t.robot_radius);
    assert!((45.0..65.0).contains(&t.robot_height), "height {}", t.robot_height);
    assert!((8.0..11.0).contains(&stride), "stride {stride}");
    let walk = Input { stick: Vec2::Y, ..Input::default() };
    let across = |y: f32, top: f32| sim::Box3 { min: Vec3::new(-200.0, y, 0.0), max: Vec3::new(200.0, y + 10.0, top) };

    let tall = Arena { boxes: vec![across(80.0, 40.0)], ..Arena::default() };
    let mut s = standing(&t);
    run_until(&mut s, &t, &tall, walk, 15.0, |_| false);
    println!("tall wall at y 80: he stopped at y {:.2}", s.pos.y);
    assert!(s.pos.y <= 80.0 - t.robot_radius + 0.05 && s.pos.y > 80.0 - t.robot_radius - 2.0, "y {}", s.pos.y);

    let low = Arena { boxes: vec![across(40.0, stride - 2.0)], ..Arena::default() };
    let mut s = standing(&t);
    let highest = std::cell::Cell::new(0.0f32);
    run_until(&mut s, &t, &low, walk, 15.0, |s| s.pos.y > 70.0 || { highest.set(highest.get().max(s.pos.z)); false });
    let highest = highest.get();
    println!("low wall at y 40, {:.1} high: he reached y {:.2}, highest z {highest:.2}", stride - 2.0, s.pos.y);
    assert!(s.pos.y > 70.0 && highest < 0.01, "y {} z {highest}", s.pos.y);
}

#[test]
fn his_cameras_frame_a_giant() {
    let Some((t, _)) = devastator() else { return };
    let (follow, aim) = (&t.robot_camera, &t.weapon_camera);
    println!("follow: {:.1}..{:.1} m, height {:.1}, fov {}; aim: {:.1}..{:.1} m, side {:.1}, height {:.1}",
        follow.min_dist, follow.max_dist, follow.height_offset, follow.fov, aim.min_dist, aim.max_dist, aim.side_offset, aim.height_offset);
    // The follow camera stands well back and looks up at him; the aim camera is closer, over his shoulder.
    assert!(follow.min_dist > 60.0 && follow.default_elevation < 0.0);
    assert!(aim.max_dist < follow.min_dist && aim.side_offset > 5.0 && aim.height_offset > follow.height_offset);
    // His footfalls' shake (`FOOTFALL_SHAKE`, at his place) is whole where his camera stands: the data's
    // 47 m full and 90 m gone never reached a camera 105 m off.
    let step = t.shakes.get(&tf2_core::giant::FOOTFALL_SHAKE).expect("the footfall shake");
    println!("footfall shake: whole to {:.0} m, gone at {:.0} m", step.full_dist, step.max_dist);
    assert!(step.full_dist > follow.max_dist + 20.0, "his steps shake his own camera");
}

#[test]
fn his_pods_are_read_from_the_boss_mission() {
    let dir = tf2_core::install::dir();
    if !dir.exists() {
        return;
    }
    let pods = tf2_core::giant::read_pods(dir);
    for p in &pods {
        println!("{:#010x} {:?} attribute {:#010x}, {} mounts, model {}, fire sound {:#010x}", p.def.name, p.def.kind, p.def.attribute, p.mounts.len(), p.object.is_some(), p.def.fire_sound);
        for e in &p.effects {
            println!("    effects {:#010x}: flash {} trail {} blast {} emitters {} flight sounds {:x?} blast sounds {:x?}", e.name, e.muzzle_flash.is_some(),
                e.trail.is_some(), e.blast.is_some(), e.emitters.len(), e.flight_sounds, e.blast_sounds);
        }
        for m in &p.mounts {
            println!("    bone {:#010x} barrels {:?}", m.bone, m.barrels);
        }
    }
    let counts: Vec<usize> = pods.iter().map(|p| p.mounts.len()).collect();
    assert_eq!(counts, [4, 3, 2]);
    let attributes: Vec<u32> = pods.iter().map(|p| p.def.attribute).collect();
    assert_eq!(attributes, [0xb90f_6d35, 0xc935_6fe6, 0x2fd6_d8f3]);
}

/// The bug bombs' eruption (`giant::Lob`) [assumed port design]: every bomb leaves
/// out along the launcher's axis, climbs 40-95 m, and comes down on its own spot of the ring round his feet,
/// spread all round him.
#[test]
fn bug_bombs_erupt_and_rain_round_him() {
    use tf2_core::giant::{Lob, ERUPTION_RING};
    let root = Vec3::new(10.0, -20.0, 0.0);
    let launcher = root + Vec3::new(4.0, -6.0, 48.0);
    let mut seed = 0x1234_5678u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut quarters = [0; 4];
    let mut flights = Vec::new();
    for _ in 0..200 {
        // The launcher tipped back and to his right, as his body might hold it.
        let axis = Vec3::new(0.4, -0.5, 0.77).normalize();
        let mut lob = Lob::erupt(launcher, axis, root, &mut random);
        let reach = (lob.landing - root).truncate().length();
        assert!(reach >= ERUPTION_RING[0] - 0.01 && reach <= ERUPTION_RING[1] + 0.01, "lands {reach} m out");
        assert!(lob.velocity.normalize().dot(axis) > 0.999, "leaves along the launcher: {:?}", lob.velocity);
        let mut top = f32::MIN;
        let bearing = (lob.landing - root).truncate();
        quarters[(usize::from(bearing.x < 0.0)) * 2 + usize::from(bearing.y < 0.0)] += 1;
        let mut lowest_down = f32::MAX;
        while lob.age < lob.flight - 1e-4 {
            let (_, to) = lob.step(DT.min(lob.flight - lob.age)).expect("in flight");
            top = top.max(to.z);
            if lob.velocity.z < 0.0 {
                lowest_down = lowest_down.min(to.z);
            }
        }
        assert!(lob.position.distance(lob.landing) < 0.05, "ends {:?}, meant {:?}", lob.position, lob.landing);
        assert!(lowest_down >= lob.landing.z - 0.05);
        assert!(top > launcher.z + 38.0 && top < launcher.z + 97.0, "climbs {} m", top - launcher.z);
        flights.push(lob.flight);
    }
    println!("quarters {quarters:?}, flights {:.2}..{:.2} s", flights.iter().cloned().fold(f32::MAX, f32::min), flights.iter().cloned().fold(0.0, f32::max));
    assert!(quarters.iter().all(|&q| q > 30), "all round him: {quarters:?}");
}

/// His missiles fly a top attack (`giant::TopAttack`) [assumed port design]: each leaves its pod near
/// straight up and leaning away from his body, climbs at least 45 m over the higher end, and comes
/// down steeply on its point; a locked one's point moving (the target walking) still ends on it.
#[test]
fn his_missiles_climb_then_dive_on_their_point() {
    use tf2_core::giant::{TopAttack, TOP_ATTACK_CLIMB};
    let root = Vec3::new(0.0, 0.0, 0.0);
    let mut seed = 0x9e37_79b9u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    // A pod on his right shoulder; points near and far ahead, behind him, and one walking.
    let pod = Vec3::new(9.0, 2.0, 46.0);
    for (point, walk) in [
        (Vec3::new(0.0, 25.0, 1.5), Vec3::ZERO),
        (Vec3::new(30.0, 300.0, 1.5), Vec3::ZERO),
        (Vec3::new(-20.0, -120.0, 1.5), Vec3::ZERO),
        (Vec3::new(10.0, 150.0, 1.5), Vec3::new(8.0, 0.0, 0.0)),
    ] {
        let mut dive = TopAttack::launch(pod, point, root, &mut random);
        let leaves = dive.direction();
        assert!(leaves.z > 0.9, "leaves near straight up: {leaves:?}");
        assert!(leaves.truncate().dot((pod - root).truncate()) > 0.0, "leans out from his body: {leaves:?}");
        let mut top = f32::MIN;
        let mut target = point;
        while dive.age < dive.flight - 1e-4 {
            target += walk * DT;
            dive.track(target);
            let (from, to) = dive.step(DT.min(dive.flight - dive.age)).expect("in flight");
            top = top.max(to.z);
            assert!((to - from).length() < 12.0, "no jump: {} m in a step", (to - from).length());
        }
        assert!(dive.position.distance(target) < 0.05, "ends {:?}, meant {target:?}", dive.position);
        assert!(top > pod.z + TOP_ATTACK_CLIMB[0] - 1.0, "climbs {} m", top - pod.z);
        let comes = dive.direction();
        assert!(comes.z < -0.7, "dives steeply: {comes:?}");
        println!("to {point:?}: flight {:.2} s, top {:.0} m over the pod, in at {:.0} deg", dive.flight, top - pod.z, (-comes.z).asin().to_degrees());
    }
}

/// Only the gatling pods on the side he shoots to fire (`giant::pods_facing`) [assumed port design]:
/// his pods as placed in the game (`giant: gatling pods` in a capture): two on his shoulders, far out,
/// and two on his back legs, as `x` right and `y` ahead.
#[test]
fn only_the_pods_on_the_shot_side_fire() {
    use tf2_core::giant::pods_facing;
    let pods = [Vec3::new(-27.0, 16.8, 40.0), Vec3::new(26.7, 17.5, 40.0), Vec3::new(-10.6, -17.1, 15.0), Vec3::new(11.0, -17.2, 15.0)];
    assert_eq!(pods_facing(&pods, Vec3::Y), [true, true, false, false], "ahead: the front pods");
    assert_eq!(pods_facing(&pods, -Vec3::X), [true, false, true, false], "left: the left pods");
    assert_eq!(pods_facing(&pods, Vec3::X), [false, true, false, true], "right: the right pods");
    assert_eq!(pods_facing(&pods, -Vec3::Y), [false, false, true, true], "behind: the leg pods");
    assert_eq!(pods_facing(&pods, Vec3::new(-1.0, 1.0, 0.0)), [true, false, false, false], "ahead-left: the left shoulder");
    assert_eq!(pods_facing(&pods, Vec3::new(0.94, -0.32, 0.0)), [false, true, false, true], "right and back: never the left leg across his body");
    assert_eq!(pods_facing(&pods, Vec3::new(-0.2, -0.98, 0.0)), [false, false, true, true], "behind: not the left shoulder");
    assert_eq!(pods_facing(&pods, Vec3::Z), [true; 4], "straight up: all");
}

/// The bombs leave from spots spread over his back (`giant::vent_spots`) [assumed port design]:
/// within the disc, apart from each other, on both his sides and turned with his facing.
#[test]
fn bombs_leave_from_all_over_his_back() {
    use tf2_core::giant::{vent_spots, ERUPTION_VENT_SPREAD};
    let vent = Vec3::new(3.0, -7.0, 50.0);
    for forward in [Vec3::Y, Vec3::new(1.0, 1.0, 0.0).normalize(), -Vec3::X] {
        let spots = vent_spots(vent, forward, 12);
        let right = Vec3::new(forward.y, -forward.x, 0.0);
        assert!(spots.iter().all(|s| (*s - vent).truncate().length() <= ERUPTION_VENT_SPREAD + 0.01));
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[i + 1..] {
                assert!(a.distance(*b) > 3.0, "spots {a:?} and {b:?} too close");
            }
        }
        let sides: Vec<f32> = spots.iter().map(|s| (*s - vent).dot(right)).collect();
        assert!(sides.iter().any(|&x| x > 8.0) && sides.iter().any(|&x| x < -8.0), "both sides: {sides:?}");
    }
}

/// His damage shows in stages (`giant::damage_stage`) [assumed port design]: one more under each of
/// 75, 50 and 25 % of his health, and a stage is left only once he mends 5 % over it.
#[test]
fn his_damage_comes_in_stages_and_mends_back_with_a_margin() {
    use tf2_core::giant::{damage_stage, pod_breaks_at};
    assert_eq!(damage_stage(1.0, 0), 0);
    assert_eq!(damage_stage(0.74, 0), 1);
    assert_eq!(damage_stage(0.4, 1), 2);
    assert_eq!(damage_stage(0.1, 2), 3);
    assert_eq!(damage_stage(0.0, 0), 3, "a big hit skips stages");
    assert_eq!(damage_stage(0.77, 1), 1, "mended just over 75 %: still broken");
    assert_eq!(damage_stage(0.81, 1), 0, "mended past 80 %: whole again");
    assert_eq!(damage_stage(0.52, 3), 2, "mended past the last stage's margin");
    let stages: Vec<usize> = (0..6).map(pod_breaks_at).collect();
    assert_eq!(stages, [1, 2, 3, 1, 2, 3], "pods break in turns");
}

/// His missiles always come down (`giant::missile_point`) [assumed port design]: on what his look
/// meets within reach, else on the ground under his look's line at the reach.
#[test]
fn his_missiles_come_down_ahead_when_his_look_meets_nothing() {
    use tf2_core::giant::{missile_point, MISSILE_REACH};
    let eye = Vec3::new(0.0, 0.0, 57.0);
    let near = Vec3::new(0.0, 120.0, 0.0);
    assert_eq!(missile_point(eye, Vec3::Y, near, 0.0), near, "a point in reach is kept");
    let far = missile_point(eye, Vec3::Y, Vec3::new(0.0, 5000.0, 57.0), 0.0);
    assert!((far - Vec3::new(0.0, MISSILE_REACH, 0.0)).length() < 1e-3, "level: on the ground at the reach ({far})");
    let up = Vec3::new(0.0, 1.0, 1.0).normalize();
    let sky = missile_point(eye, up, eye + up * 5000.0, 0.0);
    assert_eq!(sky.z, 0.0, "up at the sky: still on the ground");
}

/// The boss mission's effects and sounds he is dressed in are found (`giant::read_dressing`,
/// `read_pods`) [data].
#[test]
fn his_dressing_and_pod_sounds_are_read() {
    let dir = tf2_core::install::dir();
    if !dir.exists() {
        return;
    }
    let dressing = tf2_core::giant::read_dressing(dir);
    assert!(dressing.pod_smoke.is_some() && dressing.sparks.is_some(), "pod smoke and sparks");
    assert!(dressing.pod_blast.is_some() && dressing.plume.is_some(), "pod burst and plume");
    let pods = tf2_core::giant::read_pods(dir);
    let kind = |k: usize| pods.iter().find(|p| p.kind == k).expect("every pod kind");
    assert_eq!(kind(tf2_core::giant::GATLING).def.overheat_sound, tf2_core::giant::OVERHEAT_SOUND);
    let blasts = |k: usize| kind(k).effects.iter().flat_map(|e| e.blast_sounds.clone()).collect::<Vec<_>>();
    assert!(blasts(tf2_core::giant::MISSILE).contains(&tf2_core::giant::MISSILE_BLAST_SOUND));
    assert!(blasts(tf2_core::giant::BUG_BOMB).contains(&tf2_core::giant::BOMB_BLAST_SOUND));
    let lit: Vec<bool> = pods.iter().map(|p| p.flash.is_some()).collect();
    assert_eq!(lit, [true, false, false], "the gatling pods' muzzle light; the launchers have none");
    let lights = tf2_core::point_light::load_flash_lights(dir, "devastator").expect("his lights");
    assert_eq!(lights.len(), 3, "every pod kind flashes, the launchers with the gatling's light");
}
