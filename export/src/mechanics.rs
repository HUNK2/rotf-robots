//! `<pack>.json`'s `mechanics`: each mechanic's numbers for this robot, as the reference uses
//! them, for the formulas in `docs/MECHANICS.md`: on foot, the jump, the car, flight and its
//! boundary, hover, climbing, the dodge, melee, the weapons, damage and health, the special, the
//! cameras and the sound.

use serde_json::{Map, Value, json};
use tf2_core::character::CharacterData;
use tf2_core::{aim, camera, climb, flybound, lockon, repair, special_types, special_weapon, weapons};
use tf2_core::formats::hash::crc32;
use tf2_core::formats::lxb::{DataFile, Node};
use tf2_core::melee::ClipBlast;
use tf2_core::sim::{self, JumpType};
use tf2_core::tuning::{CameraTuning, RootCurve, Tuning};

use crate::describe::value;
use crate::names::Names;
use crate::run::num;

/// Every thing of the game's type `kind` in `files`, as the game stores it, each once.
pub(crate) fn things<'a>(files: &'a [DataFile], kind: &str) -> Vec<Node<'a>> {
    // Each stored object is walked once: the data reaches the same object by many links, and some
    // lie deep (Grindor's energon cannon's blast template), so the walk is not cut off by depth.
    // A struct and its first field share a place, so the type is part of what is seen.
    fn walk<'a>(node: Node<'a>, kind: &str, seen: &mut std::collections::HashSet<(usize, Option<&'a str>)>, out: &mut Vec<Node<'a>>) {
        if let Some(at) = node.offset()
            && !seen.insert((at, node.type_name()))
        {
            return;
        }
        if node.type_name() == Some(kind) {
            out.push(node);
            return;
        }
        for (_, field) in node.fields() {
            walk(field, kind, seen, out);
        }
        for item in node.items() {
            walk(item, kind, seen, out);
        }
    }
    let mut out = Vec::new();
    for file in files {
        walk(file.root(), kind, &mut std::collections::HashSet::new(), &mut out);
    }
    out
}

/// `things` as JSON, the same thing reached twice written once.
fn things_json(files: &[DataFile], kind: &str, names: &Names) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for thing in things(files, kind) {
        let v = value(thing, names, 0);
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// CombatRepair's heal zones (`HealAreaEffect`) in the pack, each once, with the form whose model
/// holds it. The pack nests the vehicle's `Instance` inside the robot's, so a zone reached under an
/// `Instance` that sits inside another is the vehicle's, and the rest are the robot's. [data]
fn heal_zones(pack: &[DataFile], names: &Names) -> Vec<Value> {
    // (the zone, the innermost Instance above it, how many Instances are above it)
    fn walk<'a>(node: Node<'a>, depth: u32, instances: &mut Vec<u32>, out: &mut Vec<(Node<'a>, usize)>) {
        if depth > 24 {
            return;
        }
        if node.type_name() == Some("HealAreaEffect") {
            out.push((node, instances.len()));
            return;
        }
        let instance = node.type_name() == Some("Instance");
        if instance {
            instances.push(node.label().unwrap_or(0));
        }
        for (_, field) in node.fields() {
            walk(field, depth + 1, instances, out);
        }
        for item in node.items() {
            walk(item, depth + 1, instances, out);
        }
        if instance {
            instances.pop();
        }
    }
    let mut found = Vec::new();
    for file in pack {
        walk(file.root(), 0, &mut Vec::new(), &mut found);
    }
    let mut zones: Vec<Value> = Vec::new();
    let entry = |thing: Node, vehicle: bool| -> Option<Value> {
        let script = thing.get("LuxScriptName").and_then(Node::int)? as u32;
        Some(json!({
            "trigger": names.name(script),
            "vehicle": vehicle,
            "radius": num(thing.get("healRadius").and_then(Node::float)?),
            // `specialHealingType` true is Ratchet's slot (1), false Long Haul's (0). [game 007ebf00]
            "slot": usize::from(thing.get("specialHealingType").and_then(Node::int)? != 0),
        }))
    };
    // A zone met under a nested Instance anywhere is the vehicle's; the robot's are never met so.
    let vehicle: Vec<Value> = found.iter().filter(|(_, depth)| *depth >= 2).filter_map(|(n, _)| entry(*n, true)).collect();
    for (thing, _) in &found {
        let Some(as_vehicle) = entry(*thing, true) else { continue };
        let v = if vehicle.contains(&as_vehicle) { as_vehicle } else { entry(*thing, false).unwrap_or(Value::Null) };
        if !zones.contains(&v) {
            zones.push(v);
        }
    }
    zones
}

/// A name hash as the data's name, or null for 0.
fn name_or_null(names: &Names, hash: u32) -> Value {
    if hash == 0 { Value::Null } else { json!(names.name(hash)) }
}

/// One explosion a trigger sets off, for the special's and the punches' blasts.
pub(crate) fn blast(b: &ClipBlast, names: &Names) -> Value {
    let mut v = explosion(&b.explosion, names);
    v["trigger"] = json!(names.name(b.name));
    v["node"] = json!(names.name(b.node));
    v["follows_node"] = json!(b.attach);
    v["rumble"] = b.rumble.map_or(Value::Null, |r| json!({
        "kind": r.kind,
        "duration": num(r.duration),
        "strength": num(r.strength),
        "scale_type": r.scale_type,
        "distance_near": num(r.distance_near),
        "distance_far": num(r.distance_far),
    }));
    v
}

/// An explosion's numbers (a blast's, or the one a weapon's round makes where it ends).
pub(crate) fn explosion(e: &tf2_core::formats::scene::ExplosionDef, names: &Names) -> Value {
    json!({
        "delay": num(e.delay),
        "radius": num(e.radius),
        "start_scale": num(e.start_scale),
        "grow_seconds": num(e.time),
        "damage": num(e.damage),
        "min_damage": num(e.min_damage),
        "fall_off": num(e.fall_off),
        "ground_pound_override": e.pound_override,
        "damages_player": e.damage_player,
        "damages_self": e.can_damage_self,
        "damage_flags": format!("0x{:08x}", e.damage_flags),
        "knock_back_speed": num(e.knock_back_speed),
        "knock_back_angle_degrees": num(e.knock_back_angle),
        "stun_seconds": num(e.stun_time),
        "slow_seconds": num(e.slow_time),
        "dot_seconds": num(e.dot_time),
        "heat": num(e.heat),
        "camera_shake": name_or_null(names, e.camera_shake),
    })
}

/// The special ability, for MECHANICS.md's "The special": its clocks and type, what it sends in
/// each form, the weapon each form fires, the blasts its trigger sets off, the shield and melee
/// factors of types 0 and 1, the heal zones of CombatRepair and the deployed turret's data.
/// `pack` is the robot's own pack's data files.
fn special(t: &Tuning, names: &Names, attributes: Option<Node>, pack: &[DataFile]) -> Value {
    let s = &t.special;
    let forms = attributes.map(special_weapon::read_forms).unwrap_or_default();
    let form = |f: &special_weapon::SpecialForm| json!({ "weapon": name_or_null(names, f.weapon), "from_animation": f.from_anim });
    let blasts: Vec<Value> = [(false, s.script, &t.actions.spawners), (true, s.vehicle_script, &t.actions.vehicle_spawners)]
        .into_iter()
        .filter(|(_, script, _)| *script != 0)
        .flat_map(|(car, script, spawners)| {
            spawners.iter().filter(move |b| b.name == script).map(move |b| {
                let mut v = blast(b, names);
                v["vehicle"] = json!(car);
                // Where its node is at rest in the robot's (or the car's) own frame: where the
                // blast starts when nothing drawn gives the node's place.
                v["centres"] = Value::Array(b.centres.iter().map(|c| json!([num(c.x), num(c.y), num(c.z)])).collect());
                v
            })
        })
        .collect();
    let factors = |f: &special_types::Factors| json!({
        "shield": num(f.shield),
        "flight_shield": num(f.flight_shield),
        "melee": num(f.melee),
        "flight_melee": num(f.flight_melee),
    });
    let m = &t.damage.special_multipliers;
    let rates = attributes.map(repair::Rates::read).unwrap_or_default();
    json!({
        "type": s.kind,
        "cooldown": num(s.cooldown),
        "duration": num(s.duration),
        "icon": s.icon,
        "trigger": name_or_null(names, s.script),
        "vehicle_trigger": name_or_null(names, s.vehicle_script),
        "vehicle_clip": name_or_null(names, s.vehicle_anim),
        "weapon": form(&forms[0]),
        "vehicle_weapon": form(&forms[1]),
        "blasts": blasts,
        "factors": factors(&m.single),
        "multiplayer_factors": factors(&m.multi),
        "scout_speed_factor": num(special_types::SCOUT_SPEED_FACTOR),
        "starscream_special_multiplier": num(t.damage.starscream_special_multiplier),
        "flight_transfer_multiplier": num(t.damage.flight_transfer_multiplier),
        "flight_transfer_radius": num(t.damage.flight_transfer_radius),
        "heal_zones": heal_zones(pack, names),
        "healed": {
            "rate_from_long_haul": num(rates.rate[repair::SLOT_LONGHAUL]),
            "rate_from_ratchet": num(rates.rate[repair::SLOT_RATCHET]),
            "seconds_per_tic_from_long_haul": num(rates.secs_per_tic[repair::SLOT_LONGHAUL]),
            "seconds_per_tic_from_ratchet": num(rates.secs_per_tic[repair::SLOT_RATCHET]),
        },
        "turret": {
            "fire": things_json(pack, "FireTurret", names),
            "rotator": things_json(pack, "TurretRotator", names),
        },
    })
}

/// One camera's settings as `tuning::load` reads them: angles in degrees, scales in percent.
fn camera_settings(c: &CameraTuning) -> Value {
    json!({
        "fov": num(c.fov),
        "min_dist": num(c.min_dist),
        "max_dist": num(c.max_dist),
        "min_elevation": num(c.min_elevation),
        "default_elevation": num(c.default_elevation),
        "max_elevation": num(c.max_elevation),
        "user_rot_speed_z": num(c.user_rot_speed_z),
        "user_rot_speed_x": num(c.user_rot_speed_x),
        "scale_rot_speed_min": num(c.scale_rot_speed_min),
        "scale_rot_speed_max": num(c.scale_rot_speed_max),
        "face_extra_dist": num(c.face_extra_dist),
        "face_extra_dist_gain": num(c.face_extra_dist_gain),
        "climb_extra_dist": num(c.climb_extra_dist),
        "min_height_gain": num(c.min_height_gain),
        "interp_elev_gain": num(c.interp_elev_gain),
        "interp_ang_gain": num(c.interp_ang_gain),
        "interp_target_height_time": num(c.interp_target_height_time),
        "height_offset": num(c.height_offset),
        "min_speed_align_move": num(c.min_speed_align_move),
        "max_speed_align_move": num(c.max_speed_align_move),
        "align_gain": num(c.align_gain),
        "align_damp": num(c.align_damp),
        "max_speed_fov_scale": num(c.max_speed_fov_scale),
        "max_speed_dist_add": num(c.max_speed_dist_add),
        "user_rot_scale_max_speed": num(c.user_rot_scale_max_speed),
        "user_rot_gain": num(c.user_rot_gain),
        "user_rot_damp": num(c.user_rot_damp),
        "tilt_amount": num(c.tilt_amount),
        "tilt_gain": num(c.tilt_gain),
        "lean_amount": num(c.lean_amount),
        "lean_gain": num(c.lean_gain),
        "max_rot_speed": num(c.max_rot_speed),
        "side_offset": num(c.side_offset),
        "ahead_offset": num(c.ahead_offset),
        "elev_gain": num(c.elev_gain),
    })
}

/// The cameras, for MECHANICS.md's "The cameras": each form's settings at each distance level,
/// the sniper's, the heights they look at, and every camera shake and pad rumble the game has.
fn cameras(t: &Tuning, names: &Names) -> Value {
    let levels = |l: &[CameraTuning]| l.iter().map(camera_settings).collect::<Vec<_>>();
    let mut shakes: Vec<(String, Value)> = t
        .shakes
        .iter()
        .map(|(hash, d)| {
            (names.name(*hash), json!({
                "ang_horiz_degrees": num(d.ang_horiz.to_degrees()),
                "ang_vert_degrees": num(d.ang_vert.to_degrees()),
                "tilt_degrees": num(d.tilt.to_degrees()),
                "pos_horiz": num(d.pos_horiz),
                "pos_vert": num(d.pos_vert),
                "full_dist": num(d.full_dist),
                "max_dist": num(d.max_dist),
                "duration": num(d.duration),
                "half_duration": num(d.half_duration),
                "max_ang_acc_degrees": num(d.max_ang_acc.to_degrees()),
                "ang_gain": num(d.ang_gain),
                "ang_damp": num(d.ang_damp),
                "max_offs_acc": num(d.max_offs_acc),
                "offs_gain": num(d.offs_gain),
                "offs_damp": num(d.offs_damp),
            }))
        })
        .collect();
    shakes.sort_by(|a, b| a.0.cmp(&b.0));
    json!({
        "robot": levels(&t.robot_camera_levels),
        "weapon": levels(&t.weapon_camera_levels),
        "drive": levels(&t.drive_camera_levels),
        "sniper": t.sniper_camera.as_ref().map_or(Value::Null, |(c, z)| {
            let mut v = camera_settings(c);
            v["zoom_fov"] = num(z.zoom_fov);
            v["zoom_full_time"] = num(z.full_time);
            v
        }),
        "robot_height": num(t.robot_height),
        "robot_radius": num(t.robot_radius),
        "vehicle_height": num(t.vehicle_height),
        "vehicle_radius": num(t.vehicle_radius),
        "body_offset_z": num(t.drive.body_offset_z),
        "weapon_look_height": num(t.weapon_look_height),
        "shakes": Value::Object(shakes.into_iter().collect::<Map<_, _>>()),
        "rumbles": t.rumble_presets.iter().map(|p| json!({ "kind": p.kind, "duration": num(p.duration), "strength": num(p.strength) })).collect::<Vec<_>>(),
    })
}

/// The sound's code-driven numbers, for MECHANICS.md's "Sound": the gear table the engine's
/// `rpm` and `load` come from, and the battle chatter's lines and tuning.
fn sound(t: &Tuning, data: &CharacterData, names: &Names) -> Value {
    let d = &t.drive;
    let gears: Vec<Value> = t.drive_gears.iter().map(|g| json!({ "min_rpm": num(g.min_rpm), "max_rpm": num(g.max_rpm), "ratio": num(g.ratio) })).collect();
    let lines: Vec<Value> = data
        .chatter
        .iter()
        .map(|l| json!({
            "action": names.name(l.action),
            "category": names.name(l.category),
            "subcategory": names.name(l.subcategory),
            "event": names.name(l.event),
            "frequency": num(l.frequency),
        }))
        .collect();
    let chatter = names.files().iter().find_map(|f| f.root().get("BattleChatter")).map_or(Value::Null, |n| value(n, names, 0));
    json!({
        "gears": gears,
        "gearbox": {
            "engine_torque": num(d.engine_torque),
            "front_wheel_radius": num(d.front_wheel_radius),
            "mass": num(tf2_core::vehicle::mass(d)),
            "max_speed": num(d.max_speed),
            "turning_velocity_delta": num(d.turning_velocity_delta),
        },
        // The event every chatter line plays through, with the line's sample. [game]
        "voice_event": "ProgrammerSoundPlayer",
        "chatter_lines": lines,
        "chatter": chatter,
    })
}

/// The numbers the reference's code holds rather than the data: the same for every robot. [game]
fn constants() -> Value {
    json!({
        "update_seconds": num(sim::GAME_UPDATE),
        "stick_dead_squared": num(sim::STICK_DEAD_SQUARED),
        "full_stick_squared": num(sim::FULL_STICK_SQUARED),
        "move_air_control": num(sim::MOVE_AIR_CONTROL),
        "vehicle_exit_slow_squared": num(sim::VEHICLE_EXIT_SLOW_SQUARED),
        "fly_exit_long_squared": num(sim::FLY_EXIT_LONG_SQUARED),
        "climb_jump_push": num(sim::CLIMB_JUMP_PUSH),
        "jump_leave_seconds": num(sim::JUMP_LEAVE_TIME),
        "drive_line_up": num(sim::DRIVE_LINE_UP),
        "unfold_run_stick": num(sim::UNFOLD_RUN_STICK),
        "car_air_push": num(sim::CAR_AIR_PUSH),
        "fly_drop_ray": num(sim::FLY_DROP_RAY),
        "fly_wall_ray": num(tf2_core::flight::WALL_RAY),
        "fly_side_rays": tf2_core::flight::SIDE_RAYS.map(|p| [num(p.x), num(p.y), num(p.z)]),
        "flight_boundary_limit": num(flybound::BOUNDARY_LIMIT),
        "flight_ceiling_z": num(flybound::CEILING_Z),
        "flight_bounce_damping": num(flybound::BOUNCE_DAMPING),
        "flight_scrape_limit": num(flybound::SCRAPE_LIMIT),
        "flight_ramp_rate": num(flybound::RAMP_RATE),
        "flight_push_speed": num(flybound::PUSH_SPEED),
        "flight_pitch_target_down": num(flybound::PITCH_TARGET_DOWN),
        "flight_pitch_rate_down": num(flybound::PITCH_RATE_DOWN),
        "flight_wall_align_gate": num(flybound::WALL_ALIGN_GATE),
        "flight_wall_velocity_keep": num(flybound::WALL_VELOCITY_KEEP),
        "flight_wall_forward_limit": num(flybound::WALL_FORWARD_LIMIT),
        "melee_aim_stick_least": num(sim::AIM_STICK_LEAST),
        "melee_turn_least_radians": num(sim::ATTACK_TURN_LEAST),
        "melee_slide_least_squared": num(sim::SLIDE_LEAST_SQUARED),
        "melee_rise_most": num(sim::ATTACK_RISE_MOST),
        "strafe_stick_squared": num(sim::STRAFE_STICK_SQUARED),
        "strafe_twist_most_radians": num(sim::STRAFE_TWIST_MOST),
        "aim_stick_still": num(aim::STICK_STILL),
        "aim_fine_up_to": num(aim::FINE_UP_TO),
        "aim_fine_scale": num(aim::FINE_SCALE),
        "aim_sensitivity": num(aim::SENSITIVITY),
        "aim_assist_hard": num(aim::ASSIST_HARD),
        "aim_snap_rate_radians": num(aim::SNAP_RATE),
        "aim_reach": num(weapons::AIM_REACH),
        "weapon_facing_reach": num(weapons::FACING_REACH),
        "lock_keep_share": num(lockon::keep_share(&weapons::WeaponTuning::default())),
        "climb_stick_dead_squared": num(climb::STICK_DEAD_SQUARED),
        "climb_hand_swap_at": num(climb::HAND_SWAP_AT),
        "climb_default_crossfade": num(climb::DEFAULT_CROSSFADE),
        "ledge_catch_reach_radii": num(sim::CATCH_REACH_RADII),
        "ledge_catch_below": num(sim::CATCH_BELOW),
        "ledge_catch_least_drop": num(sim::CATCH_LEAST_DROP),
        "ledge_catch_facing": num(sim::CATCH_FACING),
        "hit_react_cooldown": num(sim::HIT_REACT_COOLDOWN),
        "camera_update_seconds": num(camera::UPDATE),
        "camera_sensitivity": num(camera::SENSITIVITY),
        "camera_stand_off_seconds": num(camera::STAND_OFF),
        "camera_climb_extra": num(camera::CLIMB_EXTRA),
        "camera_overhead_from": num(camera::OVERHEAD_FROM),
        "camera_overhead_full": num(camera::OVERHEAD_FULL),
        "camera_car_sweep_radius": num(camera::CAR_SWEEP),
        "camera_drive_floor": num(camera::DRIVE_FLOOR),
    })
}

/// Health and the receiving side of every hit, for MECHANICS.md's "Damage and health": the
/// robot's own attributes and the shared `Character` values the receiver reads.
fn damage(t: &Tuning) -> Value {
    let d = &t.damage;
    json!({
        "base_health": num(t.base_health),
        "regen_rate": num(t.health_regen_rate),
        "regen_delay": num(t.health_regen_delay),
        "regen_duration": num(d.regen_duration),
        "regen_interval": num(d.regen_interval),
        "vehicle_regen_multiplier": num(d.vehicle_regen_multiplier),
        "weapon_regen_multiplier": num(d.weapon_regen_multiplier),
        "invincible": d.invincible,
        "immovable": d.immovable,
        "large": d.large,
        "reaction_immunity": format!("0x{:08x}", d.reaction_immunity),
        "vehicle_multiplier": num(d.vehicle_multiplier),
        "knockback_multiplier": num(d.knockback_multiplier),
        "overdrive_multiplier": num(d.overdrive_multiplier),
        "bumblebee_stun_break": num(d.bumblebee_stun_break),
        "other_stun_break": num(d.other_stun_break),
        "slow_multiplier": num(d.slow_multiplier),
        "slow_fire_rate": num(d.slow_fire_rate),
        "slow_heat_rate": num(d.slow_heat_rate),
        "low_health_fraction": num(d.low_health_fraction),
    })
}

/// Climbing: the climbable test's sizes, `modeClimb` and the climb clips, for MECHANICS.md's
/// "Climbing". Null for a robot whose pack has no `ClimbSet`.
fn climb(t: &Tuning) -> Value {
    if !t.rule_sets.contains_key(&crc32(b"ClimbSet")) {
        return Value::Null;
    }
    let named = |id: u32| named(t, id);
    let hands = |stem: &str| ["L", "R"].map(|hand| named(crc32(format!("Wall{hand}_{stem}").as_bytes())));
    let c = &t.climb;
    json!({
        "radius": num(t.robot_radius),
        "height": num(t.robot_height),
        "body_offset": num(climb::body_offset(t.robot_height)),
        "reach": num((t.robot_radius + 2.0) * std::f32::consts::SQRT_2),
        "min_height": num(t.jump.standing.height),
        "pull_up_dist": num(c.pull_up_dist),
        "edge_limit_dist": num(c.edge_limit_dist),
        "arrival_deceleration": num(c.arrival_deceleration),
        "catch_clip": named(climb::catch_clip(t, true)),
        "catch_clip_ground": named(climb::catch_clip(t, false)),
        "clips": {
            "idle": hands("Idle"),
            "climb_up": hands("ClimbUp"),
            "climb_down": hands("ClimbDown"),
            "shim_left": hands("ShimLeft"),
            "shim_right": hands("ShimRight"),
            "pull_up": hands("PullUp"),
        },
        "jump_up_clip": named(crc32(b"Wall_JumpUp")),
        "jump_off_clip": named(crc32(b"Wall_JumpOff")),
    })
}

/// The flying form: `modeFly`, for MECHANICS.md's "Flight". Null for a robot that cannot fly.
/// The AI's own numbers are left out.
fn fly(t: &Tuning) -> Value {
    if !t.forms.can_fly {
        return Value::Null;
    }
    let f = &t.forms.fly;
    json!({
        "helicopter": f.helicopter,
        "robot": f.robot,
        "can_do_stunts": f.can_do_stunts,
        "l_stick_deadzone": num(f.l_stick_deadzone),
        "thrust": num(f.thrust),
        "turbo_thrust": num(f.turbo_thrust),
        "lateral_thrust": num(f.lateral_thrust),
        "lift": num(f.lift),
        "max_speed": num(f.max_speed),
        "max_turbo_speed": num(f.max_turbo_speed),
        "max_turbo_time": num(f.max_turbo_time),
        "max_turbo_cooldown_time": num(f.max_turbo_cooldown_time),
        "post_turbo_deceleration": num(f.post_turbo_deceleration),
        "brake_duration": num(f.brake_duration),
        "brake_cooldown": num(f.brake_cooldown),
        "brake_speed_scale": num(f.brake_speed_scale),
        "braking_speed": num(f.braking_speed),
        "bullet_slow_speed_percent": num(f.bullet_slow_speed_percent),
        "turn_speed": num(f.turn_speed),
        "max_turn_speed": num(f.max_turn_speed),
        "quick_turn_speed": num(f.quick_turn_speed),
        "turn_lag": num(f.turn_lag),
        "turn_restore_lag": num(f.turn_restore_lag),
        "pitch_lag_auto_level": num(f.pitch_lag_auto_level),
        "pitch_strength": num(f.pitch_strength),
        "max_pitch": num(f.max_pitch),
        "min_pitch": num(f.min_pitch),
        "max_roll": num(f.max_roll),
        "max_roll_quick": num(f.max_roll_quick),
        "roll_strength": num(f.roll_strength),
        "quick_roll_lag": num(f.quick_roll_lag),
        "helicopter_turbo_turn_factor": num(f.helicopter_turbo_turn_factor),
        "stunt_move_button_press_time": num(f.stunt_move_button_press_time),
        "stunt_move_stick_threshold_x": num(f.stunt_move_stick_threshold_x),
        "stunt_move_stick_threshold_y": num(f.stunt_move_stick_threshold_y),
        "extent_roll": num(f.extent_roll),
        "extent_pitch": num(f.extent_pitch),
    })
}

/// The hover: `modeHover`, for MECHANICS.md's "Hover". Null for a robot that cannot fly.
fn hover(t: &Tuning) -> Value {
    if !t.forms.can_fly {
        return Value::Null;
    }
    let h = &t.forms.hover;
    json!({
        "max_hover_time": num(h.max_hover_time),
        "move_acceleration": num(h.move_acceleration),
        "max_move_speed": num(h.max_move_speed),
        "hover_deceleration": num(h.hover_deceleration),
        "max_lean_degrees": num(h.max_lean_degrees),
        "lean_strength": num(h.lean_strength),
        "hover_done_particles_pct": num(h.hover_done_particles_pct),
    })
}

/// The car: `modeDrive` and the vehicle avatar, for MECHANICS.md's "The car". Null for a robot
/// that cannot drive.
fn car(t: &Tuning) -> Value {
    if !t.forms.can_drive {
        return Value::Null;
    }
    let d = &t.drive;
    let inertia = tf2_core::vehicle::inertia(d);
    json!({
        "max_speed": num(d.max_speed),
        "trigger_filter_strength": num(d.trigger_filter_strength),
        "throttle": num(d.throttle),
        "engine_torque": num(d.engine_torque),
        "turn_speed": num(d.turn_speed),
        "max_turn_speed": num(d.max_turn_speed),
        "max_turbo_speed": num(d.max_turbo_speed),
        "max_turbo_time": num(d.max_turbo_time),
        "max_turbo_cooldown_time": num(d.max_turbo_cooldown_time),
        "turbo_time_upgrades": d.turbo_upgrades[0].map(num),
        "turbo_cooldown_upgrades": d.turbo_upgrades[1].map(num),
        "turbo_throttle_boost": num(d.turbo_throttle_boost),
        "post_turbo_deceleration": num(d.post_turbo_deceleration),
        "braking_deceleration": num(d.braking_deceleration),
        "min_reverse_brake_time": num(d.min_reverse_brake_time),
        "max_reverse_speed": num(d.max_reverse_speed),
        "post_reverse_throttle": num(d.post_reverse_throttle),
        "skid_out_deceleration": num(d.skid_out_deceleration),
        "wheel_drift_factor": num(d.wheel_drift_factor),
        "drift_steering_factor": num(d.drift_steering_factor),
        "min_drift_time": num(d.min_drift_time),
        "min_drift_turbo_speed": num(d.min_drift_turbo_speed),
        "drift_turbo_time": num(d.drift_turbo_time),
        "drift_turbo_boost_factor": num(d.drift_turbo_boost_factor),
        "max_drift_turbo_speed": num(d.max_drift_turbo_speed),
        "bullet_slow_speed_percent": num(d.bullet_slow_speed_percent),
        "front_axle_length": num(d.front_axle_length),
        "rear_axle_length": num(d.rear_axle_length),
        "long_axle_length": num(d.long_axle_length),
        "front_wheel_radius": num(d.front_wheel_radius),
        "rear_wheel_radius": num(d.rear_wheel_radius),
        "render_wheel_radius": num(d.render_wheel_radius),
        "min_render_wheel_offset": num(d.min_render_wheel_offset),
        "max_render_wheel_offset": num(d.max_render_wheel_offset),
        "wheel_friction": num(d.wheel_friction),
        "suspension_length": num(d.suspension_length),
        "suspension_k": num(d.suspension_k),
        "suspension_damp": num(d.suspension_damp),
        "suspension_wt_scale": num(d.suspension_wt_scale),
        "mass": num(tf2_core::vehicle::mass(d)),
        "inertia": [num(inertia.x), num(inertia.y), num(inertia.z)],
        "body_half_size": [num(d.body_half.x), num(d.body_half.y), num(d.body_half.z)],
        "body_offset_z": num(d.body_offset_z),
        "avatar_height": num(t.vehicle_height),
        "angular_vel_damp_x": num(d.angular_vel_damp_x),
        "angular_vel_damp_y": num(d.angular_vel_damp_y),
        "max_angular_vel": [num(d.max_angular_vel_x), num(d.max_angular_vel_y), num(d.max_angular_vel_z)],
        "collision_forward_dot": num(d.collision_forward_dot),
        "exit_to_run_min_speed": num(d.exit_to_run_min_speed),
        "exit_to_run_deceleration": num(d.exit_to_run_deceleration),
        "exit_to_run_turn_speed_degrees": num(d.exit_to_run_turn_speed),
        "exit_to_idle_min_speed": num(d.exit_to_idle_min_speed),
        "exit_to_idle_deceleration": num(d.exit_to_idle_deceleration),
        "exit_to_idle_turn_speed_degrees": num(d.exit_to_idle_turn_speed),
    })
}

/// A clip by its id, as `clips` writes it (`Set.Id`), or null when the pack lacks it.
fn named(t: &Tuning, id: u32) -> Value {
    t.actions.clips.get(&id).map_or(Value::Null, |c| json!(format!("{}.{}", c.set, c.id)))
}

/// The dodge: the dash mode's cooldown, the dodge set's angles and its clips, for MECHANICS.md's
/// "The dodge". Null for a robot without `DodgeFront`, which the dodge states need.
fn dodge(t: &Tuning) -> Value {
    if !t.actions.clips.contains_key(&crc32(b"DodgeFront")) {
        return Value::Null;
    }
    let clips = |prefix: &str| {
        json!({
            "front": named(t, crc32(format!("{prefix}DodgeFront").as_bytes())),
            "back": named(t, crc32(format!("{prefix}DodgeBack").as_bytes())),
            "left": named(t, crc32(format!("{prefix}DodgeLeft").as_bytes())),
            "right": named(t, crc32(format!("{prefix}DodgeRight").as_bytes())),
        })
    };
    json!({
        "cooldown": num(t.dash_cooldown),
        "front_degrees": num(t.actions.dodge_forward),
        "back_degrees": num(t.actions.dodge_back),
        "dash_seconds": num(sim::DODGE_DASH_SECONDS),
        "clips": clips(""),
        "hover_clips": clips("Hover_"),
    })
}

/// Melee's shared numbers: the search's weights and the character's reach, the upgrades, and the
/// ground punch's damage, for MECHANICS.md's "Melee". Each attack's own are `clips[].attack`.
fn melee(t: &Tuning) -> Value {
    let m = &t.melee;
    json!({
        "angle_degrees": num(m.ang_range),
        "best_dist": num(m.best_dist),
        "angle_weight": num(m.angle_weight),
        "dist_weight": num(m.dist_weight),
        "distance": num(m.distance),
        "height": num(m.height),
        "upgrades": m.upgrades.map(|row| row.map(num)),
        "ground_pound_damage": num(t.ground_pound_damage),
        "ground_pound_damage_weak": num(t.ground_pound_damage_weak),
    })
}

/// The weapons' shared numbers: how the camera stick turns the aim in weapon mode (`AimAssist`),
/// and the car weapon's wait, for MECHANICS.md's "Weapons". Each weapon's own are
/// `weapons[].numbers`.
fn weapons(t: &Tuning) -> Value {
    let a = &t.aim;
    json!({
        "aim": {
            "lowest_sensitivity": num(a.lowest_sensitivity),
            "highest_sensitivity": num(a.highest_sensitivity),
            "acceleration": num(a.acceleration),
            "min_pitch": num(a.min_pitch),
            "max_pitch": num(a.max_pitch),
            "speed_reduction": num(a.speed_reduction),
            "speed_acceleration": num(a.speed_acceleration),
            "look_assist_turn_rate": num(a.look_assist_turn_rate),
            "target_box": a.target_box.map(num),
            "default_max_range": num(a.default_max_range),
            "snap_cone": num(a.snap_cone),
            "snap_cone_elevation": num(a.snap_cone_elevation),
        },
        "strafe_speed": num(t.strafe_speed),
        "vehicle_weapon_wait": num(t.vehicle_weapon_wait),
    })
}

/// A one-off clip's name as `clips` writes it, or null when the robot has none.
fn clip(curve: &RootCurve, name: &str) -> Value {
    if curve.forward.is_empty() { Value::Null } else { json!(name) }
}

/// `names` holds the robot's pack's data files first, `pack_files` of them, then the shared ones;
/// `attributes` is the name of the robot's `characterAttributes` block.
pub fn mechanics(t: &Tuning, data: &CharacterData, names: &Names, pack_files: usize, attributes: &str) -> Value {
    let block = names.files().iter().find_map(|f| f.root().get(&format!("characterAttributes{attributes}")));
    let pack = &names.files()[..pack_files.min(names.files().len())];
    let walk = &t.locomotion;
    let on_foot = json!({
        "turn_speed_degrees": num(t.moving.turn_speed),
        "max_speed": num(t.moving.max_speed),
        "walk_run_speed": num(t.moving.walk_run_speed),
        "move_stick": num(walk.move_stick),
        "run_stick": num(walk.run_stick),
        "walk_speed": num(walk.walk_speed),
        "run_speed": num(walk.run_speed),
        "idle_blend": num(walk.idle_blend),
        "walk_blend": num(walk.walk_blend),
        "run_blend": num(walk.run_blend),
        "stop_clip": clip(&walk.stop, "IdleStandSet.Run2Idle"),
        "land_run_clip": clip(&walk.land_run, "WalkSet.Jump_LandRun"),
        "big_land_run_clip": clip(&walk.big_land_run, "WalkSet.BigJump_LandRun"),
        "long_fall_seconds": num(walk.long_fall),
    });
    let types: Vec<Value> = [
        ("standing", JumpType::Standing),
        ("moving", JumpType::Moving),
        ("high_standing", JumpType::HighStanding),
        ("long", JumpType::Long),
        ("fall", JumpType::Fall),
        ("climb", JumpType::Climb),
    ]
    .into_iter()
    .map(|(name, kind)| {
        let k = sim::jump_kind(t, kind);
        // The fall is the same type as the standing jump, entered with no launch.
        let height = (kind != JumpType::Fall).then(|| sim::jump_height(t, kind));
        json!({
            "type": name,
            "jump_type": format!("{:08x}", sim::jump_type_hash(kind)),
            "height": height.map_or(Value::Null, num),
            "launch_speed": height.map_or(Value::Null, |h| num((2.0 * t.gravity * h).sqrt())),
            "speed": num(k.speed),
            "control": num(k.control),
            "turn_speed_degrees": num(k.turn_speed),
        })
    })
    .collect();
    let jump = json!({
        "gravity": num(t.gravity),
        "types": types,
        "takeoff_clip": clip(&walk.takeoff, "JumpSet.Jump_Takeoff"),
        "in_air_speed": num(walk.in_air_speed),
        "climb_up_height": num(t.jump.climb_up_height),
        "ground_punch_acceleration": num(t.ground_punch_acceleration),
        "ground_punch_speed_limit": num(t.ground_punch_speed_limit),
        "weapon_dilation_start": num(t.jump.weapon_dilation_start),
        "weapon_dilation": num(t.jump.weapon_dilation),
        "weapon_dilation_rate": num(t.jump.weapon_dilation_rate),
        "strafe_speed": num(t.strafe_speed),
    });
    json!({
        "constants": constants(),
        "on_foot": on_foot,
        "jump": jump,
        "car": car(t),
        "fly": fly(t),
        "hover": hover(t),
        "climb": climb(t),
        "dodge": dodge(t),
        "melee": melee(t),
        "weapons": weapons(t),
        "damage": damage(t),
        "special": special(t, names, block, pack),
        "cameras": cameras(t, names),
        "sound": sound(t, data, names),
    })
}
