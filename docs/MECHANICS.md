# The mechanics as formulas

What the reference does to the robot each update, written so a native port can build the host's
controller from it without reading Rust. Every number is either a robot's own, in `mechanics` of
`<pack>.json` (written `on_foot.max_speed`, `jump.gravity`...), or one of the game's constants
in `mechanics.constants` (`constants.move_air_control`...). The reference code is named for each
part; read it where this leaves a question.

How sure each part is goes in brackets, as in the code: **[game]** read from the game's code or
data, **[trace]** measured on recordings of the running game, **[assumed]** a guess.

So far: on foot, the jump, the car, flight and hover. Climbing, the dodge, melee, weapons and
damage are to come; until then their notes in `docs/` and the reference code are the source.

## Before anything: the update

The game updates every `constants.update_seconds` (0.032 s); clips tick at 1/60 s. Several rules
below are a share of a gap closed **per update**, not per second. A host running at another rate
turns a per-update share `c` into the share for a step of `dt` seconds as

    share(dt) = 1 - (1 - c) ^ (dt / 0.032)

which gives the same result per second. The reference does this everywhere it uses a share.

Space: x right, y forward, z up, metres. Yaw 0 faces +y and a positive yaw turns left; the facing
is `(-sin yaw, cos yaw)`. The stick is in world space, length up to 1; it is **centred** when its
squared length is at most `constants.stick_dead_squared` (0.01).

Turning toward a direction, used throughout: take the shortest way round from the yaw to the
direction's yaw and turn by at most `rate * dt`. [game]

## On foot

Reference: `core/src/sim.rs`, `step_move` and `step_pace`. [game] unless marked.

He is in one of two motion modes on foot, which the control states choose (`control`): standing
(`stateIdle`, mode `Stand`) and moving (`stateMove`, mode `Move`).

**Turning.** In `Move`, when the stick is not centred, he turns toward the stick at
`on_foot.turn_speed_degrees` per second. Standing does not turn him.

**Gait.** With `d` the stick's length (at most 1): run if `d > on_foot.run_stick`, walk if
`d > on_foot.move_stick`, else idle. Standing is always idle. `run_stick` is null for a robot with
no run clip (Devastator): he never runs.

**Speed comes from the clips.** He has no speed proportional to the stick: each gait plays a clip
whose root motion sets the pace, `on_foot.walk_speed` and `on_foot.run_speed` (the walk and run
clips' root travel over their length). Changes of pace blend:

- The paces form a stack of layers. A new pace goes on top at weight 0, and each update every
  layer's weight grows by `dt / blend` up to 1 (`on_foot.idle_blend`, `walk_blend`, `run_blend`;
  a one-off clip uses its own blend from `clips`).
- The speed is folded from the bottom up: `speed = 0`, then for each layer
  `speed += (own - speed) * weight`, where `own` is the layer's pace (0 idle, walk or run speed,
  or a one-off clip's root speed over this update).
- Once a layer reaches weight 1, the layers under it are dropped.
- Which pace goes on top: idle from a run goes through the stop clip (`on_foot.stop_clip`); idle
  from anything else fades to idle; a landing into a run (`land_run_clip`, or `big_land_run_clip`
  after `long_fall_seconds` of falling) plays out before the walk or run takes over; a one-off
  that has played out hands straight over to the walk, the run or nothing.

**Velocity.** Along the ground it is the facing times the speed. While the ground flag is clear
(stepping off an edge before the fall starts), `Move` closes only `constants.move_air_control` of
the gap to that each update, from the velocity he had.

**Devastator** is the exception. In the reference, every one of his clips moves him by its own
root motion, and his walk turns him at his turn clips' rate (90 degrees in 4 s), not at
`turn_speed_degrees`. Both are the port's own design, not read from the game [assumed]
(`core/src/giant.rs`, `docs/devastator.md`). His `mechanics` are the game's data as for anyone.

**Gravity** always acts: `z += vz * dt - g * dt^2 / 2`, `vz -= g * dt`, with `g = jump.gravity`.

## The jump

Reference: `core/src/sim.rs`, `enter_jump_mode` and `step_jump`; `docs/bumblebee-mechanics.md`,
"JumpMode". [game] unless marked.

### Which jump

When the control state machine enters the jump or fall state, the mode picks a type
(`jump.types[].type`, and the value it writes to `C_JUMP_TYPE`, `jump_type`), in this order:

1. The climb set is playing: `climb`. His vertical speed is set to 0 and he is pushed
   `constants.climb_jump_push` along his facing, on top of his velocity.
2. The ground punch set is playing: no launch.
3. He was in the car (or getting out of it) and on the ground: `high_standing` if his speed along
   the ground squared is at most `constants.vehicle_exit_slow_squared`, else `long`. He keeps the
   car's speed.
4. He was flying: `long` if his speed squared is at least `constants.fly_exit_long_squared`, else
   `fall`. Neither launches.
5. The fall set is playing, or he is not on the ground: `fall`, no launch (walking off an edge).
6. The stick is centred: `standing`.
7. Otherwise `moving`. If he was in `Move` with the stick past `constants.full_stick_squared`, his
   velocity along the ground is first set to exactly `on_foot.max_speed` along his facing, so the
   take-off speed does not depend on where in the stride the run clip was. From weapon mode it is
   set to `jump.strafe_speed` the way he was going.

### The launch

    vz = sqrt(2 * jump.gravity * height)

with the type's `height` (null: no launch). This is `launch_speed`. Note that the heights are not
each type's own field in the data: `standing` is the running jump's height times 1.2, both jumps
out of the car use the long jump's height. `height` is already that.

### In the air, each update

1. **What the stick asks for.** If the stick is not centred: turn toward it at the type's
   `turn_speed_degrees`, and ask for `stick * speed`. A `long` jump with the stick centred asks for
   `speed` along the way he is already moving. Otherwise nothing.
2. **The clip's own push** [trace]: for `standing` and `moving`, the take-off clip's root speed
   (`jump.takeoff_clip`) while it plays, then `jump.in_air_speed`, along his facing.
3. **Closing the gap.** `wanted = asked + push`; the velocity along the ground closes the type's
   `control` of the gap to `wanted` each update. With the stick centred `wanted` is 0, so the
   take-off speed bleeds away at that rate. `control` is 0 for the climb jump: nothing steers it.
4. **Gravity** as on foot. The vertical speed is never steered.

The control state cannot leave the jump until the mode has run `constants.jump_leave_seconds`.

### Landing

On touching down the control state machine leaves the jump (`control`). Going into `Move`, his
speed along the ground is capped at `on_foot.walk_run_speed`, keeping its direction; into weapon
mode at `jump.strafe_speed`. The run then builds up again through the paces above. The landing
clip is the big one after `on_foot.long_fall_seconds` of falling (`animation_rules`).

The "on the ground" flag the control states read trails the body by one update: he stays in the
jump for the update he touches down and leaves on the next. [trace]

### The ground punch

While a ground punch state plays in the air, an extra downward pull grows by
`jump.ground_punch_acceleration` each update (per update, not per second: the original is tied to
its frame rate) until gravity plus the pull reaches `jump.ground_punch_speed_limit`.

### Slow motion while aiming in the air

With the weapon set playing in a jump, a time factor `f` (1 at the start of each jump) moves
toward `jump.weapon_dilation` at `jump.weapon_dilation_rate` per second once he rises or falls
slower than `jump.weapon_dilation_start` (and while he stays above the height where that began),
and back toward 1 otherwise. Gravity is multiplied by `f`; when `f` changes, `vz` is multiplied by
`sqrt(f_new / f_old)` so the rest of the rise keeps its height; what the stick asks for and the
turn are multiplied by `sqrt(f)`. So he moves as if time ran at `sqrt(f)`: half speed at 0.25. He
faces his aim and does not turn to the stick. The clips play on at their own rate.

## The car

Reference: `core/src/sim.rs` (`step_drive`, `drive_update`, `drift_step`, `step_unfold`),
`core/src/driving.rs`, `core/src/vehicle.rs`; notes `docs/driving.md` and `docs/physics.md`.
The numbers are `car` (null for a robot that cannot drive). [game] unless marked.

The car is in two layers, and a port can take them apart:

- **The drive rules** say how fast the car may go and what it asks of the wheels: the speed caps,
  the turbo, the brake and reverse, the slide, the steering. They act on the body's velocity
  directly. A host that drives the car with its own vehicle physics keeps these and feeds them in.
- **The chassis** is the game's own wheel and spring physics. A host with its own vehicle system
  may use that instead; how far its handling then differs is the port's to say.

The car runs in whole updates of 0.032 s whatever the host's frame rate: several rules have the
update's length built in (the steering angle, the engine's push past the cap). The reference
keeps a clock and runs as many updates as have come due. [game/trace]

### Getting in and out

The control states change him into the car (`stateDriveEnterA`, `B`; `control`). The car
pushes from the update after the drive mode starts, while the change-of-form clip still plays;
the clip's root travel is not applied. In the first half of the change (`Trans_R2V_A`, or its
running version) he stays in `Move`, turning toward the stick, until the stick lines up with the
way he is moving (cosine at least `constants.drive_line_up`) or is centred. [game] [trace]

Out of the car on the ground he unfolds (mode `Unfold`). If the stick is longer than
`constants.unfold_run_stick` as it starts, he unfolds into a run: while his speed is over
`exit_to_run_min_speed` it falls by `exit_to_run_deceleration` per second, and he turns toward
the stick at `exit_to_run_turn_speed_degrees`. Otherwise the same with the `exit_to_idle_`
numbers. His velocity is his facing times that speed. Out of the car with a jump is the jump
section's rule 3.

### The drive rules, each update

1. **Trigger.** The vehicle trigger `x` (0 to 1) is filtered:
   `filtered += (x - filtered) * trigger_filter_strength * dt` when `trigger_filter_strength * dt`
   is under 1 and over 0, else `filtered = x`. It starts at 1 when the drive mode starts.
2. **The slide.** While the drift button is held and a wheel is on the ground: the speed along the
   ground falls by `skid_out_deceleration * dt`, and every tyre slides (below).
3. **Turbo.** Pressing turbo (the jump button in the car) when it is not running and its cooldown
   is over starts it for `max_turbo_time` (times the time upgrade). When it runs out a cooldown of
   `max_turbo_cooldown_time` (times the cooldown upgrade) starts. Upgrade levels 1 to 3 pick from
   `turbo_time_upgrades` and `turbo_cooldown_upgrades`; level 0 is 1. The clocks run outside the car
   too.
4. **Reverse.** When the brake is up and he is not rolling backward, reversing ends.
5. **The speed cap**, the first of these that applies, on the speed along the ground ("set the
   speed" keeps the direction):
   - turbo running: at most `max_turbo_speed`;
   - drift turbo running: at most `max_drift_turbo_speed`;
   - reversing: at most `max_reverse_speed`;
   - braking (brake down, a wheel on the ground): the speed when the brake went down is
     remembered and falls by `braking_deceleration * dt` each update, and the speed is set to it.
     At 0 the car stops dead: velocity 0, the wheels stop, the engine is cut, and the brake held
     for `min_reverse_brake_time` more starts reversing;
   - otherwise `max_speed * filtered`, so the trigger scales the top speed, not the push. After a
     turbo the cap starts at `max_turbo_speed` (after a drift turbo at `max_drift_turbo_speed`) and
     falls by `post_turbo_deceleration` per second until it meets this one.

   The turbo's cap and the normal one are multiplied by a status scale: the damage
   slow's multiplier while it runs, else `bullet_slow_speed_percent` while slowed by bullets,
   else 1.5 during the Decepticon scout's special, else 1.
6. **The engine's throttle**, the first that applies: 0 when stopped by the brake; `throttle *
   turbo_throttle_boost` in turbo; `throttle * drift_turbo_boost_factor` in drift turbo;
   `-throttle` reversing; while braking `throttle` when rolling forward and `-throttle` when
   backward; `post_reverse_throttle` when rolling backward without the brake; else `throttle`.
7. **Steering.** The front wheels turn by
   `(max_turn_speed - (max_turn_speed - turn_speed) * clamp(speed / max_speed, 0, 1)) * -stick.x * dt`
   radians (`stick.x` positive to the right). It has the update's length in it: at 0.032 s
   Bumblebee's front wheels turn 0.256 rad at a standstill and 0.128 at top speed.
8. **The drift turbo** (its own step, after the tyres push and before the springs): while the drift button is held and the sideways
   speed (velocity along the car's right) is over `min_drift_turbo_speed`, a charge builds; after
   `min_drift_time` of it the turbo is armed. Letting go of the drift button while armed (and the
   standard turbo not running) starts it for `drift_turbo_time`. Losing the sideways speed resets
   the charge; the standard turbo running throws an armed one away.

### The chassis, each update

The body is a box (`body_half_size`, its centre `body_offset_z` above his root) of `mass` and
`inertia`. Each wheel hangs at a mount: `(∓front_axle_length/2, long_axle_length/2, 0)` at the
front and `(∓rear_axle_length/2, -long_axle_length/2, 0)` at the back, in the car's frame.

The order within an update: the body is realigned (step 7's, from the update before), the wheels
find the ground (1), the drive rules run, then steps 2 to 6, the drift turbo, then 7 to 9.

1. **Each wheel finds the ground**: a ray 15 long, straight down the support normal, from
   `avatar_height` above its mount. With `extension = hit distance - 2 * avatar_height`, the wheel
   is on the ground when the surface's normal z is over 0.1 and `extension` is at most
   `1.5 * rest length - min(2 * vz * dt, 0)`; its spring length is then `rest length - extension`.
   Off the ground the length relaxes back to the rest length at 10 per second.
2. **No wheel down:** the stick pushes the velocity by
   `(right * stick.x + forward * stick.y) * constants.car_air_push * dt`, the yaw rate decays by
   `yaw_rate * dt`, the angular velocity is damped (step 6), and a pitch or roll past 10 degrees is
   turned back toward level at 10 degrees a second. Steps 3 to 6 do not run; 7 to 9 do.
3. **Hops are removed:** with the car upright (its up's z over 0.9) and moving along its up, 95% of
   that part of the velocity is taken off.
4. **Each wheel on the ground pushes.** With `v` the body's velocity at the wheel
   (`velocity + angular × arm`), `f` the wheel's rolling direction and `s` its sideways one (the
   front ones turned by the steering), `w` its spin and `r` its radius (`front_wheel_radius`,
   `rear_wheel_radius`):

       slip = v - f * (w * r)
       lat  = dot(slip, s)    (0 when under 0.01)
       lon  = dot(slip, f)
       force = -s * lat * wheel_friction * a  -  f * lon * b
       w += (r * lon + engine_torque * throttle) / r^2 * dt

   `a` and `b` are 1, or `wheel_drift_factor` and `drift_steering_factor` while the car slides.
   The force goes on the body at the wheel's mount (adding torque), unless stopped by the brake.
   So sideways slip is opposed outright and the push comes only from the wheel spinning faster
   than the ground; in a steady pull each wheel pushes `engine_torque * throttle / r`.
5. **Weight transfer** sets each wheel's rest length:
   `suspension_length + sign(dot(mount, Δv_local)) * suspension_length * (1 - suspension_wt_scale) * min(|Δv|, 1)`,
   with `Δv` this update's change of the velocity along the ground in the car's frame. Near top
   speed (at least 95% of `max_speed`) with the speed steady, the front and back are tilted by the
   gearbox's rev instead (`vehicle.rs`, `transfer_weight`).
6. **Angular damping**: the angular velocity's x and y are multiplied by `angular_vel_damp_x` and
   `angular_vel_damp_y`, and all three are kept within `max_angular_vel`.
7. **The support normal** turns toward the ground's: the plane through three grounded wheels'
   contact points (with four, the lowest is left out) at a rate of 2 a second; one or two wheels'
   own normal at 10; none, straight up at 1. When it has turned (dot with the last under 0.98) the
   body is realigned to it about its centre.
8. **Springs, gravity and the step.** Each grounded wheel's spring pulls the body's point
   `arm = mount - (0, 0, avatar_height)` toward the wheel's anchor
   `centre + support * arm + normal * length`: with `d` that gap, the force is
   `suspension_k * d / max(|d|, 1)` (none when `|d|^2` is at most 0.001), at the arm. The damping
   takes `(v · u) u * suspension_damp / 60` off the velocity, with `u = d / (|d| max(|d|, 1))`.
   The game splits the update into pieces of at most 1/60 s for these two, weighting each by its
   share. Then velocity and angular velocity are multiplied by 0.9995, the forces (tyres,
   springs, gravity `mass * jump.gravity` down) are integrated over the update, and the body
   meets the world: against static geometry the part of the velocity going into a contact is
   removed, with no bounce.
9. **A head-on hit** (a contact approaching faster than 0.5 whose normal dotted with the car's
   forward is under `collision_forward_dot`) stops the wheels' spin.

What is not settled: the spring and contact model is compared with the original's recordings and
still misses some of the recorded cases (`docs/driving.md`, "CE suspension comparison"); against a
level's collision the contact solver is not finished. The drive rules above are matched with the
recordings.

## Flight

Reference: `core/src/flight.rs` (`Fly`: `step_mode`, `steer_helicopter`, `cap_speed`,
`meet_walls`, `advance`), `core/src/sim.rs` `step_fly`; notes `docs/flight-*.md`. The numbers
are `fly` (null for a robot that cannot fly). Angles in the data are degrees, turn rates radians a
second. [game] unless marked.

Three kinds of flyer take different paths: **jets** (Starscream, Breakaway, Jetfire),
**helicopters** (`fly.helicopter`: Grindor, pack `blackout`), and **robot flyers** (`fly.robot`,
which also have `helicopter` set: Megatron and Optimus in their flight packs, who fly as
themselves). Every rule below is the jets'
unless it says otherwise.

### The pieces every flyer uses

- `lag(value, target, rate)`: `value += (target - value) * rate * dt`, or straight to the target
  when `rate` is not positive or `rate * dt` is 1 or more.
- The stick is shaped: inside `l_stick_deadzone` it is 0, the rest is stretched back to the full
  range, then squared keeping its sign. `lateral` is the shaped x, `pitch_in` the shaped y.
- The body's angles are a heading (about z), a pitch (about x, positive nose up) and a roll
  (about y), drawn as `Rz(heading) Rx(pitch) Ry(roll)`. The nose is
  `(-sin heading cos pitch, cos heading cos pitch, sin pitch)`.
- **No gravity** while flying: the flying form's body has none.
- **The ground under it**: a ray `constants.fly_drop_ray` long, straight down from 2 m ahead of
  the body along the nose. Its height over the ground, allowing for the body's size, is
  `S = pos.z + 2 sin(pitch) - ground - 2 (fly.extent_roll |sin roll| + fly.extent_pitch |sin pitch|)`.
- The **ramp** goes from 0 at the start of a flight up to 1 at 2 a second.
- The **turbo** (the jump button) starts when it is not running and its cooldown is over, for
  `max_turbo_time` (times the car's time upgrade); the cooldown is `max_turbo_cooldown_time`
  (times the cooldown upgrade). The **air brake** (the brake button) otherwise starts when ready,
  for `brake_duration`, with a cooldown of `brake_cooldown`, and remembers the speed it began at.
  The timers count down after the update.

### A jet, each update

1. **Thrust**: `vel += nose * thrust * dt`, or `turbo_thrust` while the turbo runs.
2. **A stunt** (only with `can_do_stunts`): the action button arms stunts for
   `stunt_move_button_press_time`. Armed, the stick past `stunt_move_stick_threshold_x` to a side
   rolls that way, past `stunt_move_stick_threshold_y` back loops up, forward loops down (only
   more than 10 over the ground). A stunt's clip then flies it: the stick steers nothing until the
   clip ends, a roll's roll angle dies away at 10 a second, and its first update puts the
   velocity along the nose at `max_speed`. When the clip ends the heading is taken from where the
   nose points and the jet goes on along it at `max_speed`.
3. **Across the nose**: 95% of the velocity at right angles to the nose is taken off, each update.
4. **Close to the ground** it is pushed up: with `h` 1.85 (2.85 over the surfaces the level marks
   for it), when `S` is under `h`, `vz += 150 * (1 - S / h) * dt`.
5. **Pitch**: toward `-max_pitch * pitch_in` at `pitch_lag_auto_level`, kept within `max_pitch`
   and within 0.49 pi either way. So the stick forward dives and back climbs. A forward stick
   is first multiplied by the ramp.
6. **Roll and turn**: the roll goes toward `max_roll * lateral` at `roll_strength`; the yaw
   rate toward `turn_speed * lateral` at `turn_lag` (back toward 0 at `turn_restore_lag` when the
   stick is centred, and at once to 0 when the stick is pushed against the turn); then
   `heading -= yaw_rate * dt`. Holding the tight-turn button (the weapon-mode trigger) instead:
   the roll goes to `max_roll_quick` toward the side the stick first leaned, at
   `quick_roll_lag`, and the turn's target is `quick_turn_speed`.
7. **Walls**: three rays `constants.fly_wall_ray` long along the nose, from the body and from
   `constants.fly_side_rays` either side of it (each sees what it found the update before). A
   wall (normal z within 0.1 of level) closer than 10:
   - two walls whose normals are at right angles or more (a corner): an eased push (each axis at
     3 a second) toward straight up, at `500 * dt`, when the stick is back past 0.25, straight
     down when forward past 0.25, else none; and 5% off the speed along the ground;
   - otherwise the nearest wall: with `u` the nose laid along the wall and `c = u · n`, an eased
     push along `u` of `max((1 - 0.1 d) * 100 * (1 - c), 500) * dt`, and 95% of the velocity into
     the wall taken off.
8. **The speed cap**, on the whole velocity, the first that applies: turbo running, at most
   `max_turbo_speed`; air brake running, a remembered speed falls by `braking_speed` per second
   but not under `max_speed * brake_speed_scale`, and the speed is cut to it; just after a turbo
   (unless beside a wall), the speed is SET to a remembered speed that starts at
   `max_turbo_speed` and falls by `post_turbo_deceleration` per second until it meets
   `max_speed`; else at most `max_speed`. The turbo and normal caps take the same status scale as
   the car's, with `fly.bullet_slow_speed_percent`.
9. **Move**: `pos += vel * dt`; it cannot go under the ground.

### A helicopter or robot flyer, each update

No thrust along the nose. The move stick (shaped as above: `q0` across, `q1` along) pushes it,
the camera stick (`turn` across, `lift` up and down, dead-zoned but not squared) turns and lifts
it. Only while no clip drives it. In order:

1. In turbo: `q1` is at least 1 when pushed forward or centred, and `q0`, `turn` and `lift` are
   multiplied by 0.85.
2. Within 2 m of the ground (`S`, 3 m over marked surfaces) on ground steeper than 0.5 in normal z
   it goes no lower: a downward `lift` becomes 0 and `vz` 0.
3. Sticks at rest: the velocity along the ground keeps 95% each update (not in turbo); with no
   lift, `vz` keeps 95%.
4. The hold button (not in turbo, not a robot flyer) stops it: velocity keeps 95% each update,
   and the move stick's x turns it at half rate instead of pushing.
5. Otherwise it is pushed: across the nose by `lateral_thrust * q0 * dt`, along it (level) by
   `thrust * q1 * dt` (`turbo_thrust` in turbo, times `helicopter_turbo_turn_factor` when moving
   against the push), and `vz -= fly.lift * lift * dt` (the camera stick up gives a negative
   `lift`, so it climbs).
6. Roll toward `max_roll * q0` at `roll_strength`. Yaw rate toward `turn_speed * turn` at
   `turn_lag` (`turn_restore_lag` when `|turn|` is under 0.05), kept within `max_turn_speed`;
   `heading -= yaw_rate * dt`.
7. Pitch: holding, the move stick turns the nose up and down at up to 90 degrees a second
   (eased at 2 a second) between `min_pitch` and `max_pitch`; otherwise toward
   `-max_pitch * q1` (pulled back) or `min_pitch * q1` (pushed forward) at `pitch_strength`.
8. The speed cap and the move as the jet's 8 and 9.

### Getting in and out

The flying form starts from the trigger held (`control`: `stateFlyEnterA` and the rest), keeping
the velocity it had; its pitch and roll start level.

**A clip's pull.** Some clips carry a traction vector `c` (their `traction` events in
`clips[].events`, `axes`): the take-off clips and the stunts. While `c` is not zero, after the update
above, each axis of the velocity is pulled toward the clip's root motion turned to the heading:
`vel += c * (root_velocity - vel)`, and the place moves by the difference. The stick steers nothing
meanwhile (a helicopter's steering and a jet's stunt check both wait for `c` to be zero). This is
what lifts Starscream off the ground in the run below: his speed climbs and dips with the take-off
clips, then thrust and the cap hold it at `max_speed`. Letting go starts the change back
(`stateFlyExit*`), and he falls: the jump's rule 4 takes a long jump at speed, else a fall.

### The flight boundary

Each zone has a boundary the flyers stay inside. Outside it a jet is turned and bounced back
in, and a helicopter or robot flyer stopped against it and pushed back (`core/src/flybound.rs`,
`docs/flight-boundary.md`). Those rules are not written out here yet.

## Hover

Reference: `core/src/sim.rs` `step_hover`; `docs/flight-hover.md`. The numbers are `hover`.
[game]

The hover states (`control`) hold a flyer in the air in its robot form, in mode `Hover`. No
gravity. Each update:

1. With the stick centred, the velocity is multiplied by `hover_deceleration` (per update).
   Otherwise `vel += stick * move_acceleration * dt`, and its length is kept within
   `max_move_speed`.
2. With weapon mode's aim, he faces it.
3. While the clip changing him into hover plays, its root motion drives the velocity's up
   axis.
4. `pos += vel * dt`; he cannot go under the floor.
5. After `max_hover_time` (when it is not negative; -1 is no limit) the mode sends the message
   that takes the control state out of hover, and he falls.

`max_lean_degrees`, `lean_strength` and `hover_done_particles_pct` are the lean drawn and the
effect; the reference does not use them for movement.

## Checking

Play `export/runs/basics.json` (`rotf-export --run`, `docs/FORMAT.md` "Reference runs") through
the port and compare. For Bumblebee: the run settles at `run_speed` (24); on the update of the
running jump `vz` is 25.338, the `launch_speed` of 26.458 less one update of gravity, and the speed
along the ground goes 24, 24.536, 25.207, 25.910 as it closes 0.038 of the gap to the asked 35 plus
the take-off clip's push. Starscream, flying level with the stick centred, holds exactly 45
(`fly.max_speed`): thrust adds 1.6 an update and the cap takes it off again. In the car his speed along the ground settles near 37.5, not at the
`max_speed` of 35: the cap is applied at the start of each update and the engine then pushes for
that update past it (all four wheels at `engine_torque * throttle / r` on a body of 5000, about
81 m/s², 2.6 per update).
