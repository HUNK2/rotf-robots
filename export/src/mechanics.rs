//! `<pack>.json`'s `mechanics`: each mechanic's numbers for this robot, as the reference uses
//! them, for the formulas in `docs/MECHANICS.md`. So far on foot, the jump, the car, flight and hover.

use serde_json::{Value, json};
use tf2_core::sim::{self, JumpType};
use tf2_core::tuning::{RootCurve, Tuning};

use crate::run::num;

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

/// A one-off clip's name as `clips` writes it, or null when the robot has none.
fn clip(curve: &RootCurve, name: &str) -> Value {
    if curve.forward.is_empty() { Value::Null } else { json!(name) }
}

pub fn mechanics(t: &Tuning) -> Value {
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
    json!({ "constants": constants(), "on_foot": on_foot, "jump": jump, "car": car(t), "fly": fly(t), "hover": hover(t) })
}
