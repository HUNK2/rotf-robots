# The mechanics as formulas

What the reference does to the robot each update, written so a native port can build the host's
controller from it without reading Rust. Every number is either a robot's own, in `mechanics` of
`<pack>.json` (written `on_foot.max_speed`, `jump.gravity`...), or one of the game's constants
in `mechanics.constants` (`constants.move_air_control`...). The reference code is named for each
part; read it where this leaves a question.

How sure each part is goes in brackets, as in the code: **[game]** read from the game's code or
data, **[trace]** measured on recordings of the running game, **[assumed]** a guess.

Covered: on foot, the jump, the car, flight and its boundary, hover, climbing, the dodge, melee,
the weapons, damage and health, the special, the cameras, sound, and effects. The effects
themselves are `effects` of `<pack>.json`; the sounds are `sounds`.

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

Reference: `core/src/flybound.rs` (`Boundary::test`, `Steerback`, `wall_stop`) and
`core/src/flight.rs` (`boundary_contact`, `step_mode`); `docs/flight-boundary.md`. The robot's
numbers are in `fly`, the rest in `constants`. [game] Nobody has recorded a flyer leaving a
zone: Starscream has been seen bouncing off a boundary in the game, and that is the only check.

Each zone has a **flight boundary**, a level object, not a robot's number: a box (or, for the
ellipsoid class, a cylinder) placed by a 4x3 row-vector placement (rows 0 to 2 its half-size
axes, row 3 its centre) on an owner whose node matrix puts it in the world. The host has to give
its levels the same. The test arena has none.

**The test,** before the flight update each update, at the flyer's camera target point (its root
x and y, and the lower of its root z and its body's z less half the vehicle's height). Take the
point into the box's coordinates `(u, v, s)`, where the box runs from -1 to 1: [game]

1. Outside when `u > constants.flight_boundary_limit` (1.01), else `u < -1.01`, else `v > 1.01`,
   else `v < -1.01`, else `s > 1.01` (the ceiling), tested in that order. The face's normal points
   back in: `(-1,0,0)` for `u` too big, and so on; `(0,0,-1)` for the ceiling. **There is no
   floor.**
2. The cylinder: inside the box but `u² + v² > 1` is also outside, and the normal of any side is
   `(-u, -v, 0)`.
3. The push-back direction **V** is that normal turned into the world by the owner's matrix and
   made unit length. **N** is the unit direction from the point to the box's centre.
4. On the update he goes outside, the "armed" flag clears. While outside, the **target turn
   rate** is `±fly.quick_turn_speed`. When V points down (`V.z < constants.flight_ceiling_z`,
   -0.95: out through the top) its sign is the sign of the turn rate he has. Otherwise it is
   negative when V points to his left (`V . right < 0`).

Outside, and not in a stunt, the flight update in "A jet" or "A helicopter" is replaced by one of
these. The turbo, the brake and the speed caps still run after it.

**A jet** (neither `helicopter` nor `robot`) is turned and bounced back in. With `stick` his left
stick and `t = (stick.y, -stick.x, 0)`: [game]

1. `vel -= constants.flight_bounce_damping * (vel . t) * t` (0.9, per update, not by `dt`).
2. `d = N . stick`. When `d > constants.flight_scrape_limit` (0.95), or once armed: the turn rate
   is 0 and he is armed. Otherwise the turn rate moves toward the target at
   `constants.flight_ramp_rate` (5) a second: `rate += (target - rate) * 5 * dt`, or jumps to it
   when `5 * dt >= 1`.
3. The heading turns by `rate * dt`, `heading -= rate * dt`, wrapped to [-pi, pi).
4. The roll moves the same way toward `±fly.max_turn_speed` degrees, by the turn rate's sign (0
   counts as positive), or toward 0 when armed.
5. V not pointing down: the pitch moves toward 0 the same way. V pointing down: `vel += V *
   constants.flight_push_speed * dt` (30), and the pitch moves toward
   `constants.flight_pitch_target_down` (-30 degrees, nose down) at
   `constants.flight_pitch_rate_down` (2) a second.

**A helicopter or robot flyer** is stopped against it and turned to face in. With `right` its right
axis and `w = (-right.y, right.x, 0)` (its nose, when level): [game]

1. V pointing down (out through the top): the steering below runs with no stick push, the camera
   stick's x as the turn, and a lift of +1, so it is pushed down. The velocity is kept.
2. Otherwise, when `N . w >= constants.flight_wall_align_gate` (0.8), it is facing in: the
   steering runs with the forward stick full on and no turn.
3. Otherwise the velocity is multiplied by `constants.flight_wall_velocity_keep` (0.95, per
   update, not by `dt`). Take `d = N - w` made unit length into the body's frame as
   `(xc, yc)`. The steering runs with no stick push and a turn of `yc` when `yc <
   constants.flight_wall_forward_limit` (-0.9), else `xc`.

"The steering" is the helicopter's update ("A helicopter or robot flyer, each update") with those
inputs in place of the sticks, and no turbo boost.

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

## Climbing

Reference: `core/src/climb.rs` (`find`, `Climb::enter`, `update`) and `core/src/sim.rs`
(`step_climb`, `ledge_catch`); `docs/climbing.md`. The numbers are `climb`, which is null for a
robot whose pack has no climb set (`ClimbSet`). Checked against the 12 climbs of a recording of
Bumblebee (the numbers are at the end of `docs/climbing.md`). [game] [trace]

On the wall the clips move him: each update the climb clip's root motion, turned from his own
frame (x right, y forward, z up) into the world, is his velocity. The climb mode decides which
clip plays, holds him to the wall and decides when he leaves it. There is no gravity on the wall,
and the two jumps off it (`stateClimbJumpUp`, `stateClimbJumpOff`) are moved by their clips on
all three axes too.

### What is a wall

The climb starts from the control states (`control`): the Climb button down and the climbable
test passing. The test does not look at collision. It looks at the level's **ledge mesh**, the
2.5D map of floor heights the game keeps beside the collision (`docs/climbing.md` "What starts a
climb"); a host has to give its levels something that answers the same question. [game]

1. Start at his feet raised by `body_offset` (his `height * 24/121`: where his body shape starts
   above the ground), and look along his facing for `reach = (radius + 2) * sqrt 2`.
2. The **wall** is the first place along that line where the ground beyond is higher than the
   start. Its **bottom** is the ground before it, its **top** the ground beyond.
3. It counts when `top - bottom >= min_height` (the jump block's `jumpHeight`; anything lower is
   a ledge to jump onto), when the start is less than `radius + 2` in front of it, and when he
   faces it within 45 degrees (`facing . n <= -0.7071`, `n` the wall's normal pointing out of
   it).
4. The wall runs along neighbouring edges that stay within cos 11.5 degrees (0.98) of the same
   line. Where it turns a corner by less than 45 degrees, that end is extended by `radius + 2`.
5. The **wall frame**: x along the wall from its (extended) left end, y into the wall, z up from
   the bottom. In it, x runs from 0 to the `width`, y is `-radius` when he touches it, and z is
   the height above the bottom, up to the wall's `height` (top less bottom). Where the top's cell
   has another ledge above it, it carries an `extra` height.
6. While the test passes it is run again every update, from where he is; the climb's clip and
   the wall follow what it finds.

When he leaves the climb state with Climb still held, the test is locked until Climb is let go,
so letting go of a wall by the stick does not catch it again at once. [game]

### Catching hold

When he catches hold (`Climb::enter`): [game]

1. His velocity less its part along the wall's frame y is kept as an **arrival speed**.
2. Within `edge_limit_dist` of either side he is moved in to that distance, at `-radius` off the
   wall and 0.1 higher; a wall no wider than twice that puts him at its middle.
3. Above the pull-up line (`height - pull_up_dist`) by `over`, the arrival speed is replaced by
   `(0, 0, -sqrt(100 * over))`.
4. The catch clip plays: `catch_clip` in the air, `catch_clip_ground` from the ground (the
   climb set's own rules pick them).
5. On the first update he is put at `-radius` off the wall and turned square to it.

### Each update on the wall

`climb.rs` `update`, in this order. "Pushed" is the pad's left stick, x right and y up, past
`constants.climb_stick_dead_squared` (0.01). [game]

1. If the pull-up clip has finished, the climb ends in the pull-up's exit (below).
2. If the climbable test fails from where he is, he lets go: the bottom exit (unless he is
   pulling up).
3. The arrival speed loses `arrival_deceleration * dt` of its length. Beyond either side limit
   (x under `edge_limit_dist` or over `width - edge_limit_dist`) its part along the wall goes
   at once.
4. Held to the wall: when he is more than `radius + 0.1` off it and nothing touches his body from
   within 45 degrees of his facing, a tenth of the gap to `-radius` is closed (per update). Where
   the collision stands in front of the ledge mesh he hangs on it instead (2.1 to 2.2 off on some
   walls of the recording). [trace] Then, while the arrival speed lasts, `pos += arrival * dt`.
5. While the catch clip plays, he waits for it to finish or for the stick to be pushed, then
   hangs (`WallL_Idle`) and the rest of this list starts next update.
6. The stick's angle, 0 straight up and positive to the left:
   - **Centred:** below the pull-up line he hangs; at or above it he goes over the top.
   - **Up** (from -45 up to, not including, 45 degrees): below the pull-up line he climbs up;
     at or above it he goes over the top.
   - **Down** (under -135 or from 135 round): when his z in the wall frame is under
     `0.1 - body_offset` or he stands on the ground, the bottom exit; else he climbs down.
   - **Left** (45 up to 135) or **right** (-135 up to -45): he shimmies that way unless he is
     within `edge_limit_dist` of that end, in which case he hangs. Either way he turns square to
     the wall at half a turn a second.
   A clip already playing is not started again. Climbing up or down, the next clip (the other
   hand's, after the change in 7) starts when the one playing has finished. Nothing changes the
   clip during the pull-up.
7. **The hands.** Each clip comes as `WallL_<stem>` and `WallR_<stem>` for the leading hand
   (stems `Idle`, `ClimbUp`, `ClimbDown`, `ShimLeft`, `ShimRight`, `PullUp`). Climbing up or
   down, the hand changes once each time the clip passes `constants.climb_hand_swap_at` (0.3) of
   its length. After the catch he leads with the left.
8. The velocity is the clip's root motion. When the clip changes, the new clip's motion is
   blended in from the old clip's over the new clip's blend time
   (`constants.climb_default_crossfade`, 0.1 s, when it has none).

**Over the top:** the point above him at the top of the wall and his facing are kept. With no
`extra` above, the pull-up clip plays and the pull-up exit is sent; with an `extra`, the top is
raised by it and the jump-up exit is sent.

The pad rumbles each update: going down hums, going up or sideways is silent (its preset lasts 0
seconds). [data]

### Leaving the wall

The exits are messages to the control states, which take them (`control`): [game]

- **Pull-up:** `stateClimbPullup` plays the pull-up clip out, then he falls (onto the top, in
  the recording: he is in Move or a short fall right after, about 4.7 past the wall). As that
  state ends, the **ledge catch** below runs without asking that he be coming down.
- **Bottom:** he falls.
- **Jump up** (the exit, or Jump pressed): `stateClimbJumpUp` plays `Wall_JumpUp`, then the jump.
  With the stick pulled back, Jump plays `Wall_JumpOff` (`stateClimbJumpOff`), then the jump.
- **Climb pressed again:** he falls.

A jump that starts while the climb set plays is the climb jump: vertical speed cleared, then
`constants.climb_jump_push` (20) added along his facing, and the `climb` row of `jump.types`
(height `climbJumpOffHeight`, no control in the air). The jump up the wall is the same type.
[game; the recording has no wall jumps, so this is not checked]

**The ledge catch** (`ledge_catch`): after the jump up, while he is coming down, below the kept
top, less than `radius` past it along the kept facing, and facing within
`constants.ledge_catch_facing` (0.98) of it: a ball of his `radius` is dropped from the top plus
his `height`, at `constants.ledge_catch_reach_radii` (2.25) radii past the top along the kept
facing, down to his feet. If it lands above his feet, less than `constants.ledge_catch_below`
(1.5) above them, under his feet plus his height, and more than `constants.ledge_catch_least_drop`
(0.25) below where it started, he is put there, facing the kept way, with no velocity. The catch
after the pull-up leaves out "coming down" and "under his feet plus his height". [game]

## When a clip moves him

The dodge, the attacks, the specials and other states whose clip moves him (`control`, motion
modes `DashMode`, `AttackMode`, `FirstMode`) share this. Reference: `core/src/sim.rs`
`begin_action`, `start_clip`, `step_action`. [game]

1. The state's clip starts. When the clip before it ended in this same update, the new clip
   starts as far in as the old one ran past its end. [trace, one transition measured]
2. Each update the clip's root moves on by `dt * 60 * clip speed` ticks. Its root travel over
   that span, turned from his frame (x right, y forward) into the world, divided by `dt`, is the
   velocity the clip asks for.
3. That velocity is blended in from the one he had when the clip started, over the clip's blend
   time (0.1 s when it has none): `fade += dt / blend` up to 1, `vel.xy = lerp(before, clip,
   fade)`.
4. Only x and y: gravity and the ground act on z as on foot. The exceptions are the climb's two
   jumps and the attacks out of the car and out of flight, whose clips drive z too.
5. A clip whose root turns turns him with it (only `Wall_JumpOff` does, by 124 degrees).
   [assumed]

## The dodge

Reference: `core/src/sim.rs` `begin_action` (the `StateDash` branch), `step_action`, `stop_clip`;
`docs/bumblebee-mechanics.md`. The numbers are `dodge`, null for a robot without the dodge's front
clip. [game] [data]

1. **When:** the dodge states in `control` (the dodge button, from the states that allow it). A
   dodge state agrees to start only when the cooldown has run out and the robot has `DodgeFront`.
2. **Which way:** take his aim's heading in weapon mode, else his yaw. The stick's angle against
   it, in degrees from -180 to 180, positive to the left, picks the clip by the dodge set's own
   rules (`animation_rules`, `DashSet`):
   - front from `-front_degrees` up to, not including, `front_degrees` (45);
   - back from `back_degrees` (135) round either way;
   - right between them on the negative side, left on the positive.
   While he hovers, the same angles pick the `Hover_Dodge*` clips.
3. **Facing:** he is turned to that heading at once and kept on it until the dodge ends.
4. **The move:** the clip moves him ("When a clip moves him"). Gravity acts as on foot.
5. **Homing:** on starting a dodge his dash clock is set to 0.5 s; while it runs, homing
   missiles do not steer at him. Clip events (`DashEvent`) can set it too, for their own time.
6. **The end:** the control states leave the dodge when its clip has finished. As the dodge state
   ends, the cooldown starts at `cooldown` seconds (`groundDashCooldown`), and it counts down
   every update.

## Melee

Reference: `core/src/sim.rs` (`attack_aim`, `step_action`, `land_hits`, `play_action`,
`pass_events`), `core/src/melee.rs` (`Query`, `touch`, `upgrade_scale`); `docs/melee.md`. Each
attack clip's own numbers are its `clips[].attack` (FORMAT.md); the shared ones are `melee`.
[game] No recording has melee or health data in it: this is checked only by tests.

### The chain

The attacks are control states (`StateAttack`, motion mode `AttackMode`); the clip's events say
when what can happen (FORMAT.md, `clips[].events`): [game]

1. `attack_button_window` opens and closes a window. An attack press while it is open is kept.
   When it closes with a press kept, the state may branch: `attack_can_branch` in `control`.
   That is the chain: the next hit's state takes that connection.
2. `branch_point` raises the clip's branch count (1 at the start, 100 once finished), which the
   connections' `branch_begin`, `branch_end` test.
3. `attack_face_and_slide` sets what the attack may do from there: slide, face, follow the stick.
4. `attack_hit` switches the collision body on a node (`collision`, preset `Attack`, and the
   melee weapons' own bodies) into hitting, and back.

### Each update of an attack

Before the clip moves him ("When a clip moves him"): [game]

1. **Looking for a target,** while the attack has not hit a character and slide or face is on:
   - The direction is the stick when it is at least `constants.melee_aim_stick_least` (0.001)
     long, else his facing. When the clip's `stick_choose_target_max_degrees` is under 180 and
     the stick is further round than that, the direction is put at that limit, on the side
     **away** from the stick (the game's own arithmetic; only the slam out of the car has such a
     limit).
   - With no target kept yet, search along it ("The search") with the clip's `max_range` and
     `auto_target_search_angle_degrees`. A target found is kept for the rest of the attack and the
     chain, until an attack that hit a character is left.
2. **Toward the target,** while it is flat further away than touching (his `radius` plus, for a
   character, its radius):
   - with face on and the clip's `auto_target_turn_speed` over 0: turn to it at that rate (degrees
     a second), only when more than `constants.melee_turn_least_radians` (1 degree) off;
   - with slide on, the clip's `slide_speed` over 0, and the 3D distance between `min_range` and
     `max_range` and its square over `constants.melee_slide_least_squared` (0.1): **move his
     position** toward it by `slide_speed * dt`, at most the distance. He is not stopped at
     touching distance; the bodies' collision parts them.
3. **The stick's turn,** when no turn to a target happened this update, the clip allows the stick,
   its `turn_speed` is over 0, the stick is not centred, and, for a clip with `stop_on_hit`, no
   character has been hit: turn toward the stick at `turn_speed`.
4. With `stop_on_hit`, once a character is hit, on the ground: no more root motion along the ground
   (velocity x and y 0).
5. Off the ground his upward speed is at most `constants.melee_rise_most` (2).

### The search

A candidate at offset `d` from him: [game]

- `distance = | flat length of d - its radius |`, which must be over 0 and under the range (the
  clip's `max_range`, or `melee.distance` when it gives none); `|d.z| < melee.height`.
- `yaw` and `pitch`: how far its direction is off the direction looked along, each under the angle
  (the clip's, or `melee.angle_degrees` when it gives a negative one).
- `score = melee.dist_weight * |distance - melee.best_dist| / span(range, best_dist)
  + melee.angle_weight * (yaw + pitch) / angle`, where `span(a, b) = a - b` when that is over
  `b`, else `b`. Something that is not a character scores `dist_weight + angle_weight` more.
- The lowest score wins; the first listed wins a tie.

### The hit

A hit is a touch, not a query. While a node's `attack_hit` is on, its bodies (spheres, capsules
and boxes from `collision`, and the melee weapon's) are tested against each target's
(`Character` preset) bodies: [game]

1. A target already hit in this attack state is skipped: one hit per target per state, and the
   list empties when the state is left.
2. A foot's body hits only characters.
3. **Damage:** the clip's `damage` (a computer-run attacker's `ai_damage`, in multiplayer
   `multiplayer_damage`, when not negative). For a human player in single player, times the
   melee upgrade for this clip at the level he has it (`melee.upgrades`: the first row for the
   jump attack and `AttackFast_1` to `_3`, the second for the charged hit, the third for the slam;
   1 at level 0). Times Optimus's special's melee multiplier while it runs. For a computer-run
   attacker, times the difficulty's enemy damage scale instead.
4. **Direction** by the clip's `direction`: his forward, left, right, down or up, or from him to
   the target; flat and made length 1.
5. **Throw:** the clip's `knock_back_speed` and `knock_back_angle_degrees`; the target is thrown
   only when the damage flags have a flail or knock-back kind ("Damage and health").
6. **Flags:** `damage_flags`, or `damage_flags_small` / `damage_flags_large` by the target's size,
   less `damage_flags_excluded`.

### The ground punch's blast

The ground punch's own fist does no damage. At tick 8 its clip triggers an object spawned at the
fist: an explosion sphere (radius 18, the weak punch 15) that grows from half its size to full
over 0.15 s and hits each body once: [game] [data]

- Damage: `melee.ground_pound_damage` when the explosion's own damage is over 5 (both punches'
  are), else `melee.ground_pound_damage_weak`, also as its least damage. So both punches do
  `ground_pound_damage`.
- With a fall-off share `f` (both punches have none, -1: the whole damage everywhere), at a
  distance `x` from the middle of a sphere of radius `R`: the whole damage under `f*R`, the least
  damage from `R`, and in between `least + (damage - least) * (1 - (x - f*R) / (R - f*R))`.
- Direction flat from the blast to the target; throw 35 at 3 degrees (the weak one 25 at 3).

## Weapons

Reference: `core/src/weapons.rs` (`Weapon`, `bullet_damage`, `scattered_point`, `aim_point`,
`Missile`, `Grenade`), `core/src/aim.rs`, `core/src/lockon.rs`, `core/src/sim.rs` (the strafe
mode); `docs/weapon-mode.md`, `docs/weapon-kinds.md`, `docs/weapon-charged-shot.md`,
`docs/weapon-flamethrower.md`, `docs/special-weapons.md`. Each weapon's own numbers are its
`weapons[].numbers` (FORMAT.md), by the game's names, written `numbers.rateOfFire` below. The
aim's are `weapons.aim`. [game] No recording has weapon data in it: all of this is checked only
by tests and captures.

### Weapon mode: moving and aiming

Weapon mode is the `StateWeapon` states (`control`) in the motion mode `StrafeMode`. [game]

**Moving.** Each update the stick is turned into his frame:

1. **Stick centred** (squared length under `constants.strafe_stick_squared`, 0.0001): he stands.
   `off` = the aim's heading less his yaw, wrapped. When `|off|` is over
   `constants.strafe_twist_most_radians` (45.5 degrees) his body turns by what is past it; the spine
   takes the rest (`off` clamped to that), shared out over the links of his `SpineIK`.
2. **Stick pushed:** he faces the aim's heading at once and the spine is straight. The weapon
   set's clips move him by their root motion ("When a clip moves him"), picked by the stick's
   heading against the aim's (`F_STRAFE_ANG`, degrees, in `animation_rules`, `WeaponSet`). For a
   player the strafe mode itself moves him no other way.
3. As weapon mode starts, the aim is put on the camera's forward (unless he comes from a dodge or a
   jump, which keep theirs).

**Aiming,** each update, with the camera stick `s` (x right, y up): [game]

1. The assist's share goes toward `aim.speed_reduction` while the aim is over a target, else
   toward 1, by `aim.speed_acceleration` a second.
2. `s` counts when `|s|² > constants.aim_stick_still` (0.001). Not pushed: the ramp is 0.
3. Pushed: `m = |s|`, then `m³` when `m > constants.aim_fine_up_to` (0.8), else
   `m * constants.aim_fine_scale` (0.3125). The ramp rises toward `m` by `aim.acceleration`
   (degrees, taken as radians a second) and drops to it at once; at most 1.
4. `rate = aim.lowest_sensitivity + (aim.highest_sensitivity - aim.lowest_sensitivity) *
   constants.aim_sensitivity` (the option's default, 0.5), degrees a second.
5. `turn = ramp * share * rate * dt`. The aim's heading turns right by `(s.x / |s|) * turn`; its
   pitch moves by `(s.y / |s|) * turn`, kept within `aim.min_pitch` and `aim.max_pitch`.
6. **The pull:** with a target from the assist, the move stick pushed and `|s|² <
   constants.aim_assist_hard` (0.81), the heading moves toward the heading to the target by
   `aim.look_assist_turn_rate` a second.
7. As weapon mode starts, a target within `aim.snap_cone` degrees to either side and
   `aim.snap_cone_elevation` up or down is turned to at `constants.aim_snap_rate_radians` (4.71 radians a
   second, both angles).

**The aim point:** the first thing on the camera's forward line, from the point of the line nearest
to him out to `constants.aim_reach` (500) from the camera; or the line's far end.

### Firing and heat

Every weapon runs in whole updates of 0.032 s. Each update: [game]

1. **Asked:** the trigger is held. A launcher with `numbers.mustHoldFireTriggerToLockOn`, and a
   weapon with `numbers.hasChargedShot`, are asked when the trigger is let go after being held.
2. **Refused** when asked with the heat at `numbers.heatMeterCapacity` or over: the refusal sound,
   again at most every `numbers.rateOfFire`.
3. **Fires** when asked, more than `numbers.rateOfFire` seconds since the last round, and (unless
   `numbers.automaticFiring`) not already fired in this pull. At 0.032 s an update a rate of 0.15
   gives a round every fifth update, 0.16 s.
4. **A firing** sets the time since the last round to 0 and adds `numbers.heatMeterIncrementPerRound`
   to the heat, once whatever the number of projectiles. Under the capacity that restarts a wait
   of `numbers.heatMeterCooldownTilDecrement`; reaching it starts the overheat,
   `numbers.heatMeterOverHeatPeriod` seconds, at whose end the heat goes to 0.
5. **Bursts:** with `numbers.burstPerFire` over 1, the firings after the first come by themselves
   every `numbers.timeBetweenEachBurst`, with no trigger and no rate or heat test, while his
   weapons stay out (a car weapon while he drives or flies; a special's always).
6. **The heat falls,** when neither the overheat nor the wait runs, by
   `numbers.heatMeterDecrementPerSecond`.

A flamethrower heats by time instead: `heatMeterIncrementPerRound * dt` every update it burns.
The charged shot (`hasChargedShot`): held past `timeTilChargeStarts`, it charges over
`chargeDurationTilMaxEffect` and fires on release, with its damage or its number of firings scaled
by how far it got (`docs/weapon-charged-shot.md`).

Each projectile leaves from the next of the weapon's `barrel_nodes`. A weapon whose
`useFacingAsTarget` is set (the car's guns) aims at `constants.weapon_facing_reach` (100) straight
ahead of the barrel instead of the aim point. A car weapon fires only once
`weapons.vehicle_weapon_wait` (`vehicleWeaponTimeTilCanFire`) has passed since he changed into the car.

### A gun's round

1. **Scatter** (a player's): the aim point moves in the camera's picture plane by up to `most =
   distance from the camera * tan(vertical fov / 2) * numbers.playerNoiseRadius`, in a direction
   drawn evenly round the whole turn, by `most` times a number drawn evenly from -1 to 1. [game]
2. **The line** runs from the muzzle toward that point, `numbers.bulletRange` long. With
   `numbers.tracerFirePerRound` over 0 the whole line is traced in the update it is fired and the
   hit lands at once; the visible round then flies at `numbers.bulletSpeed` to the hit. [game]
3. **Damage:** `numbers.damage` times the charge (1 uncharged) times
   `numbers.positionalDamageMultiplier` for the part hit (head, upper body, lower body, vehicle
   body; a head counts as one only for a player's round). From `numbers.bulletDamageFallOffRange`
   (when not negative) it falls in a straight line to `numbers.minDamage` at `bulletRange`. The
   push is along the round's way, `damage * 10`; the throw, flags and special case numbers come
   with it ("Damage and health"). [game]

### A missile

Launched from the barrel along the weapon's forward at `numbers.missileStartSpeed`, sent at the
locked target or the aim point (an unguided one at a point 10 000 along that way). Each update:
[game]

1. Clocks: the wait before it steers (drawn between `minGuidanceDelay` and `maxGuidanceDelay`),
   then the time it steers before giving up (drawn between `minGuidanceOffDelay` and
   `maxGuidanceOffDelay`; negative: never), the wobble's wait and length.
2. Once it steers: the point is the target's middle while guided, else its kept point. With `c`
   the cosine between its way and the point:
   - it is armed once `c >= cos(pastTargetConeAngleActivated)`; armed, `c <= 0` (it has passed
     the target) ends the guidance and it flies straight on;
   - not wobbling: within the cone `c > cos(guidedViewAngle)` it points straight at it, else it
     turns toward it by at most `outOfViewTurnSpeed * dt`.
   - A wobble (when `minWobbleTimer` is over 0) turns it about a random axis by up to
     `maxWobbleAngle` a second, for a drawn time.
3. `speed += acceleration * dt` (times `accelFactor` once its drawn delay has passed), at most
   `missileTopSpeed`; `pos += way * speed * dt`. After `missileLifetime` it is gone.
4. While a target's dash clock runs (a dodge), a missile does not steer at it.

### The lock-on

For a `guided` launcher in weapon mode (or the car's, once it may fire): [game]

1. **Seen:** up to three targets whose picture overlaps a box of `missileLockOnBoxWidth` by
   `missileLockOnBoxHeight` pixels of a 1280 x 720 picture about its middle, ahead of the camera
   and of him, nearer than `aimAssistMaxRange`, with a clear line to their head. The best is the
   nearest, in the picture, of its point halfway across and 0.8 of the way up; last update's best
   stays first.
2. While the trigger is held (and the weapon could fire): with the candidate still seen, the time
   held goes up; at `missileCrosshairLockOnTime` it is the lock, for `lockOnBracketStayOnTime`
   after it leaves the box. With the candidate gone, the first living one seen is the new
   candidate, and the lock is dropped when its target is dead, its stay has run out, a different
   candidate is seen, or it is further off the camera's forward than
   `constants.lock_keep_share` (0.33) of the field of view (the whole of it for a car weapon).
   With the trigger up nothing changes, which is how a released missile keeps its target.
3. A lock time of 0 or less locks the best seen at once.

### A grenade

Thrown at the aim point (`fireTowardsTarget`) or along the weapon's forward, the way tilted up by
`launchArcAngle`, at `launchSpeed` (plus his own speed along it with `tackOnOwnerSpeed`). Each
update: [game]

1. After its drawn `grenadeMinLifetime` to `grenadeMaxLifetime` it goes off. A proximity grenade,
   once armed, goes off (at once, or after its drawn `proximityExplosion*Delay`) when a target's
   middle is within `proximityRadius`.
2. `vel.z += Zgravity * dt`; `pos += vel * dt`. It touches nothing for its first 0.05 s.
3. On a touch: a proximity grenade on a character goes off; a sticky one stops
   `stickyOffsetFromCollision` off the surface and stays (and goes off after its sticky time, or at
   once with none); anything else bounces, `vel -= (1 + bounciness) (vel . n) n`, and at most every
   0.25 s its velocity is also multiplied by `dampeningOnCollision`.

The blast of a missile or grenade is an explosion like the ground punch's, with the weapon's
`explosiveDamage` ("Melee", "The ground punch's blast").

## Damage and health

Reference: `core/src/damage.rs` (`resolve`, `Runtime`), `core/src/sim.rs`
(`State::receive_damage` and the reaction states); `docs/damage.md`. The numbers are `damage`.
[game] No recording has health data in it: checked only by tests.

Every hit (a fist, a round, a blast, a damage-over-time tick) reaches the one hit as a **packet**:
an amount, damage flags, a direction, a throw (speed and angle), and stun, slow and
damage-over-time times.

### Taking a hit

1. **Refused** when he cannot take damage (`damage.invincible`), when it is his own unless the
   flags have one of `0x40f0f0`, from his own team unless they have one of `0x42000000`, or from
   another team with "friends only" (bit 25).
2. The amount is multiplied, in order:
   - by `damage.overdrive_multiplier` when the attacker's overdrive runs;
   - for a computer-run receiver hit by a player, by 1 / the difficulty's enemy health scale;
   - unless "predamage" (bit 23): by a shield special's factor (Optimus's), and by
     `damage.vehicle_multiplier` in car or flight form;
   - and by 0.1 when the attacker's damage is reduced (character flag 30).
3. While a shield special runs, or for a "spirit link" hit (bit 27), it may not take the last point:
   `amount <= health - 1`.
4. Unless "damage over time" (bit 15): `health -= amount`. Under 0.1, or with "instant kill" (bit
   28), he dies: a headshot, explosion (bit 1) or ordinary death, by which the death clip is picked
   (`DeathSet`).
5. **Thrown** when the flags have a flail or knock-back kind (`0xf0000`), unless he is
   `damage.immovable`: when the packet's direction already rises more steeply than the throw's
   angle it keeps it, else its rise is replaced by the angle; times the throw's speed and
   `damage.knockback_multiplier`. That is his velocity.
6. **Statuses:** "stun" (bit 13) with a stun time starts the stun, with a break budget of
   `damage.bumblebee_stun_break` when Bumblebee stunned him, else `damage.other_stun_break`. Later
   damage takes from the budget, and the stun ends when it goes below 0. The stun time also sets
   "no car" (bit 12), "no change of form" (bit 22) and the flash (bit 6) for that long. "Slow"
   (bit 14) starts the slow (`damage.slow_multiplier`; weapons fire at `damage.slow_fire_rate` and
   heat at `damage.slow_heat_rate`). "Damage over time" ticks the packet's amount once per whole
   second crossed, over `ceil(dot time) - 0.01` seconds, the last one included. Bits 4 and 5 add
   the packet's heat to his weapon in hand or to all of them.
7. Regeneration stops and waits `damage.regen_delay` again.

### The reaction

None while a shield special runs, or for "no hit react" (bit 21). Otherwise: [game]

- Stunned (and not immune to it): the stun state (`stateStun`).
- A **full** reaction (`stateHitReact`, `HitReactionSet`) for flags in `0x170100` (melee, the
  flails, knock-back), when he is not immune to melee, not in the car, and the last full reaction
  was more than `constants.hit_react_cooldown` (4 s) ago, or at once for a flail. He turns to face
  where the hit came from.
- Otherwise a **partial** one: `HitReactPartialSet` played over what he is doing.
- Only when the control state he is in allows it (`control`).

### Regeneration

Not at a constant rate. While his health is under full, no damage over time runs, and the
character regenerates (`damage.regen_rate` over 0): the wait counts down from
`damage.regen_delay`, and once it is below zero, each step of length `i` (the update, or
`damage.regen_interval` when that is over 0) adds [game]

    3 * max_health / regen_duration³ * (i/2 - wait)² * i

(`wait` is negative by then, so this grows with time), up to full health, times
`damage.vehicle_regen_multiplier` in car or flight form and `damage.weapon_regen_multiplier` in
weapon mode. Reaching full health resets the wait.

### Death

He dies at under 0.1 health. The death state plays its clip from the root's x and y (an explosion
death from all three axes) and ends when it has finished and he is on the ground after 3 s, or
after 10 s.

## The special

Reference: `core/src/sim.rs` (`start_special`, `special_effect`, the two clocks in `advance`),
`core/src/special_types.rs`, `special_weapon.rs`, `repair.rs`, `turret.rs`;
`docs/character-specials.md` and the `special-*.md` notes. The numbers are `special`. [game]
No recording has a special in it: all of this is checked only by tests.

### Starting it

The special is the control state `StateAttackSpecial` (`control`): the special button held
(`AttackSpecial`, for the hold its connection gives). It may be entered only while the cooldown
clock is at 0 or under. On entering: [game]

1. The cooldown clock is set to `special.cooldown`. When `special.duration` is over 0 the special
   clock is set to it; with -1 there is no special clock and no end.
2. In the vehicle form the name sent is `special.vehicle_trigger`, else `special.trigger` (null:
   none). It goes to the robot's own things as a Trigger: whatever in `effects.on_models` has that
   `trigger`, every blast in `special.blasts` of that form, and the heal zone of that name.
3. The type's own start (below), by `special.type`.
4. The form's weapon (`special.weapon` on foot, `special.vehicle_weapon` in the vehicle form), when
   it has one and its `from_animation` is false, is pressed once now. With `from_animation` true,
   each `special_fire` event of the clip that plays is one press instead.

Each update both clocks go down by the step. When the special clock goes from over 0 to 0 or
under, the end runs: an Untrigger goes to both names (`trigger` and `vehicle_trigger`, whatever the
form), then the type's own end. Not in the reference: the upgrade that shortens the cooldown.

### What the trigger sets off

Each entry of `special.blasts` is an explosion put at its `node` (on the vehicle when `vehicle`)
when its trigger is sent, following the node when `follows_node`. After `delay` it is a sphere
growing from `radius * start_scale` to `radius` over `grow_seconds`, which hits each body once, as
the ground punch's blast does (Melee, "The ground punch's blast": the same damage and fall-off
rule, the flat direction, the throw `knock_back_speed` at `knock_back_angle_degrees`). Its packet
carries `damage_flags`, `stun_seconds`, `slow_seconds`, `dot_seconds` and `heat` (Damage and
health). `camera_shake` starts at its centre; `rumble`, when given, plays at the strength for the
player's distance (The cameras, "The pad's rumble"). [game] [data]

Bumblebee's: a sphere of 15 from half its size over 0.25 s on `Spine_02` (`Floor` in the car), no
damage, the stun flag (`0x2000`) and a 2.5 s stun. The effects and the sound of his special are
other things answering to the same name.

### Types 0 and 1: Optimus's shield

`special.type` 0 (on foot) and 1 (flying). The start clears his stun, flash and slow, sends
`SpecialAttackMesh` to himself (the render sets switch, `render_sets`), and starts the audio preset
`OptimusSpecial` (Sound, "The mix"). Type 1 also marks him for the damage transfer. While the
special clock runs: [game]

- A hit's amount is multiplied by `special.factors.shield` (type 0) and by `flight_shield` (type 1,
  or any hit with flag bit 27); not when the hit has bit 23, and neither for bit 28.
- No hit takes his last point: the amount is at most `health - 1`.
- No hit reactions and no heat damage reach him.
- His melee damage is multiplied by `factors.melee` (type 0) or `flight_melee` (type 1).
  `multiplayer_factors` are the multiplayer numbers.
- Type 1: a character of his side within `flight_transfer_radius` takes over his hits, times
  `flight_transfer_multiplier`. The reference has no ally to do it with.

The end sends `specialAttackEnd` and `NormalMesh` and stops the preset; type 1 clears the mark.

### Types 2, 3 and 6

- Type 2 (the scout): while it runs, every clip of every layer and of the vehicle model plays at
  `special.scout_speed_factor` (1.5) times its rate. Whether his movement speeds up too is not read;
  the reference leaves it alone. [game] [assumed]
- Type 3 (the sniper): the start sends the message `b6ff7243` to himself, the end `NormalMesh`.
  What answers the message is not read.
- Type 6 (Starscream): while the special clock runs his rounds slow what they hit, and his missiles
  do `special.starscream_special_multiplier` times their damage. [game]
- Types 4, 5 and -1 have nothing of their own: what they do is their trigger and their weapon.

### A weapon special

Megatron's gun, Jetfire's missile dump and carpet bomb, the Audi's mines and the deployed turrets
are weapons of `weapons` fired by the special. Their numbers are `weapons[].numbers` under the
game's names (`rateOfFire`, `numProjectilesPerFire`, `burstPerFire`, `timeBetweenEachBurst`,
`isSpecial`, `isVehicleModeWeapon`, `fireDummyXYAngle`, `fireDummyZAngle`...). [game]

- A press is refused while the last one's `rateOfFire` has not run out.
- A press fires one burst at once: `numProjectilesPerFire` rounds together, round `i` turned by
  entry `i` of `fireDummyXYAngle` and `fireDummyZAngle` (degrees; 0 past their end).
- Further bursts run by themselves, on whole updates: each update while bursts are left, a burst
  clock goes up by the update, and once it has reached `timeBetweenEachBurst` a burst fires and the
  gap is taken off the clock (kept at 0 or more). So Jetfire's carpet bomb drops 8 bombs 16, 16,
  15... updates apart, not every 0.5 s. A burst that is due goes on only for a weapon that
  `isSpecial`, or one that `isVehicleModeWeapon` while he is in the drive or fly mode; otherwise
  the rest is dropped (the bombs stop when he changes back).
- Each round flies as a grenade or missile of its kind (Weapons, "A missile", "A grenade").

Megatron's gun fires once, at tick 2 of `SpecialAttack_Exit`. Not settled: which way Jetfire's
dump turns its rounds (the sign of the turn is open), and where `oneOutAtATime` is enforced for the
mines. [assumed]

### The deployed turret

Blackout's and Ironhide's. The thrown body: [game]

    velocity = launchSpeed * unit(launcher's direction),  gravity zGravity   (weapons[].numbers)
    each update in the air: velocity *= 0.9995; if |velocity|² is under 2.5e-5 or over 1e5 it is 0;
                            velocity.z += gravity * dt;  place += velocity * dt

It lands on the first contact whose normal's z is over 0.5: it stops, and faces back along the
throw (its rest facing is minus the throw's direction along the ground). It plays its landing clip,
and its gun opens fire at the clip's Trigger, at the clip's end (Blackout's at 90 ticks). Ironhide's
clip sends its Trigger at 86 ticks. His deployed turret has the same parts as Blackout's, all
answering to that Trigger's name: a lock-on target, a `TurretRotator`, a `FireTurret` (`fireRate`
25, `maxFireRange` 300) and a `DeployableTurret`, beside its missile launcher where Blackout's has a
gun. [data] The reference does not fire Ironhide's turret: the notes it follows missed these parts
and took it for a decoy. That his `FireTurret` fires the missile launcher as Blackout's fires its gun
is [assumed]; a recording of his special would settle it. More than `maxTurretsAtOnce` standing: the
oldest goes.

`special.turret` is every `FireTurret` and `TurretRotator` of the pack, as the game stores them
(Megatron's pack also has a rotator: his tank's cannon). The rotator, each update, with `d` the
target less the turret and angles wrapped to plus or minus pi: [game]

    wanted yaw = atan2(-d.x, d.y);  wanted pitch = atan2(-d.z, |d.xy|)   (no target: the rest facing)
    err = wrap(wanted - angle)
    rate = 0 if |err| <= 1e-4; else rate moves toward err's side by turnAcceleration * dt,
           at most maxTurnRate, and never on the far side of 0
    move = rate * dt, but err when |move| >= |err|;  angle = wrap(angle + move)

(rates and accelerations are the data's degrees). The gun: a countdown goes down by the step while
the fire gate is open; at 0 or under it fires and is set to `1 / fireRate`. Its target: of the enemy
team's characters (in sight when `checkLineOfSight`), the first five whose flat reach
`r = |flat distance - their radius|` lies inside (`minFireRange`, `maxFireRange`), with
`|d.z| < maxFireRange` and yaw and pitch off the turret's facing under `maxFireAngle` and
`maxFireElevation`; the lowest `|r - minFireRange|` wins. A shot needs the aim within
`coneAngleToAllowFire` of the gun's facing. [game] Not settled: whether the turret's gun is
`isSpecial` (it decides whether its bursts finish).

### CombatRepair: Ratchet's and Long Haul's heal

The trigger switches on a heal zone on him (`special.heal_zones`: the zone of that `trigger` on the
model of that form), and the end switches it off: it lives as long as the special clock. Its
centre is his root. Each update: [game]

- A character is taken in when it is he or of his team, is not in already, and the distance from
  the centre to its root plus half its height is at most `radius`. (The game also tests a flag of
  the character, bit 10, whose meaning is not read.) It leaves when that distance is over `radius`;
  the team is not tested again.
- A character inside a zone of slot `s` (0 Long Haul's, 1 Ratchet's) heals by its own
  `special.healed` rate for that slot: `rate * dt` each update, or with `seconds_per_tic` over 0,
  `rate * seconds_per_tic` once each time a clock reaches the tic. Never over full health.

Long Haul's two zones are both named `CombatRepair` (25 on foot, 22 on the truck), so the same name
reaches both: which one the game switches on in which form is not read. [assumed: the form's own]

## The cameras

Reference: `core/src/camera.rs` (`Camera::step`, `update_follow`, `update_driving`,
`update_weapon`), `core/src/shake.rs`, `core/src/rumble.rs`; `docs/camera.md`. The numbers are
`cameras` and the `constants.camera_*`. The follow and driving cameras were compared with run 1
[trace]; the weapon camera, the collision and the shakes were not.

### Which camera

- In the vehicle form, car or flying: the driving camera with `cameras.drive`. In weapon mode: the
  weapon camera with `cameras.weapon` (`cameras.sniper` while sniping). Otherwise the follow camera
  with `cameras.robot`. A flyer has no camera of its own: the driving one, with its flight rules on.
- Each form has three settings, distance levels 0 to 2; a game starts on 0, and the D-pad steps the
  level round, for every form at once unless the option separates them.
- Angles in the settings are degrees and scales percent, as the data has them.
- The reference runs the camera at its own fixed step, `constants.camera_update_seconds` (0.032).
  The original ran it at the frame rate with the same gains, so faster at higher frame rates; which
  rate the values were tuned at is not known. [assumed] Between updates the reference blends its
  last two views and carries the eye on by the target's motion. [assumed]
- The stick's turn rate is scaled by `(scale_rot_speed_min + (scale_rot_speed_max -
  scale_rot_speed_min) * sensitivity) / 100`, the option's sensitivity at
  `constants.camera_sensitivity` (0.5, what the recording fits). [trace]

The target point: his root, plus `cameras.robot_height` on foot (and `weapon_look_height` more in
weapon mode), or in the vehicle form `vehicle_height + max(0, up.z * (body_offset_z -
vehicle_height / 2))`; plus the settings' `height_offset`. [game]

### Shared pieces

    spring(diff, last, gain) = last + gain * diff - 2 * sqrt(gain) * last

is a critically damped spring run once an update: `last` is last update's change, the result this
update's. `heading(v) = atan2(-v.x, v.y)`. Taking over from another camera keeps the picture: the
distance and the elevation are where the camera is, the point looked at starts on the view's line
(its height offset kept within 20), and the view's lag is how far the view then points off that
point, sprung away by `interp_elev_gain` and `interp_ang_gain` (nothing else feeds it). The follow
camera then blends to its own height over `interp_target_height_time`. [game] [trace]

The look-at lag (the "sponge"), each update, with `rise` the target point's rise this update:

    carried = sponge - rise
    change  = 0.2 * change - min_height_gain * carried
    sponge  = clamp(carried + change, -height, 20)

Its sideways part is multiplied by 0.95. The point looked at is the target point plus it.

### The follow camera, each update

1. The field of view: `spring(fov - now, last, 0.02)`.
2. The camera stays where it was, so the offset from the point looked at to it is what he walked
   (climbing, the camera moves with him). Its heading is the offset's.
3. Overhead: when last update's direction from the point to the camera rose more than
   `constants.camera_overhead_from` (0.83) and he moved, the heading is blended back toward last
   update's, all the way at `camera_overhead_full` (0.88).
4. The stick turns it by `-stick.x * user_rot_speed_z * scale * dt`. A recentre turns pi a second
   plus a tenth of what is left, until one step reaches. Climbing, a push round past 0.35 pi from
   behind him counts for less, for nothing at 0.4 pi, and past that is taken back by a tenth an
   update.
5. The extra distance: wanted `constants.camera_climb_extra` (15) climbing, dead or in the weak
   ground punch's fall; else `face_extra_dist` when he faces the camera (`facing . offset > 0`) or
   for `constants.camera_stand_off_seconds` (3.7) after a melee attack; else 0. It moves by
   `spring(wanted - extra, last, face_extra_dist_gain)`, at most 10 a second. Only for settings with
   a `face_extra_dist` or `climb_extra_dist`, or while the stand-off runs.
6. The leash: `wanted = clamp(|offset|, min_dist + extra, max_dist + extra)`, and the distance moves
   by `spring(wanted - distance, last, 0.02)`. Between the two the camera does not move: he drags it
   at the far end and pushes it at the near one. Shortened by a wall last update and still shorter:
   the distance is `|offset| + 0.2` at once.
7. The elevation is a user's part plus an automatic part. The stick: `p = -stick.y *
   user_rot_speed_x * scale * dt`; over 2 degrees a second it moves the user's part directly, kept
   in [`min_elevation`, `max_elevation`], and ends the settling. Left alone, the user's part is
   sprung (0.02) to `default_elevation`, but only until the stick is first used. The automatic
   part springs (0.03) to what keeps the sum inside the range. (The weak ground punch's fall adds 45
   degrees and asks a field of view of `min(1.5 fov, 70)`.)
8. The eye is the point plus the distance along the heading and elevation; then the collision.
9. The view turns to the point, plus the lag of "Shared pieces".

### The driving camera, each update

1. The field of view springs (0.02) to last update's wanted.
2. Smoothed speed: `rate = 0.9 * rate + 0.01 * (|velocity.xy| - s)`, `s += rate`, the velocity being
   the car's after its caps. The speed share `f` moves 15% an update toward `clamp((s -
   min_speed_align_move) / (max_speed_align_move - min_speed_align_move), 0, 1)`: for Bumblebee it
   rises only in turbo.
3. Wanted field of view: `fov * (1 + (max_speed_fov_scale / 100 - 1) * f)`.
4. The point it hangs from: the sponge as on foot; its side part moves 5 a second toward
   `side_offset` along the car's right. Neither it nor the eye goes under the floor, the car body's
   centre (`constants.camera_drive_floor` above the point in the old recordings).
5. Heading: `u = -stick.x * user_rot_speed_z * (1 - f + user_rot_scale_max_speed / 100 * f) * dt`;
   more than a quarter turn off behind the car, a push further round counts for less, and for
   nothing at 135 degrees. With `a` the angle from the camera's heading to behind the car:

       stepA = align_gain * a + (1 - align_damp) * stepA
       stepU = user_rot_gain * u + (1 - user_rot_damp) * stepU
       heading += stepA + stepU

6. Roll: `spring(lean_amount / 100 * car's roll - roll, last, lean_gain)`.
7. Distance: the follow camera's leash, measured with last update's field-of-view ratio taken out,
   plus the speed add `add = 0.9 * add + 0.1 * max_speed_dist_add * f`. The recording shows the
   distance NOT shrinking as the view widens. [trace]
8. Elevation: wanted `default_elevation - tilt_amount / 100 * car's pitch + user`, the stick adding
   `-stick.y * user_rot_speed_x * dt` to `user` (trimmed to keep the sum in range). Airborne (flat
   speed over 5 and the wheels more than 2 past their reach, until under 0.1) the tilt is at least a
   quarter of the way from the elevation to the velocity's pitch less 22.5 degrees. It moves by
   `spring(wanted - E, last, tilt_gain)`.
9. Flying (the flight rules): with the stick still, `user` halves each update; the eye's rise is
   plainly `sin E * distance`, and once the elevation has settled the field-of-view ratio scales
   the whole offset. Otherwise the eye is at least as high as the car's nose-down slope and the
   floor put it. The view keeps the line's direction: it does not turn to the car when held up.
10. The collision sweeps a sphere of `constants.camera_car_sweep_radius` (0.65) and only pulls in.

### The weapon camera, each update

It never reads the stick: the stick turns his aim (Weapons), and the camera follows the aim.

1. The field of view springs (0.02) to the settings'.
2. The point on his axis is the target point. Its height offset only springs away what the take-over
   left: `spring(-offset, last, min_height_gain)`.
3. The point it turns about sits `side_offset` to his right and `ahead_offset` ahead along the aim;
   for settings with neither (`ahead_offset` under 0.1 and `side_offset` under a quarter of his
   radius) it swings on his radius instead: back by `radius * aim.z` as he aims up. Each part moves
   by `spring(wanted - now, rate * dt, 0.5)`, its speed changing by at most 50 an update.
4. Heading: behind the aim at once; when that is more than `max_rot_speed * dt` away it swings
   instead, pi a second plus a tenth of what is left, until one step reaches.
5. Distance: the follow camera's leash, and the eye never under 1 above his feet.
6. Elevation: the aim's (`atan2(-aim.z, |aim.xy|)`) plus a lag set at the take-over and sprung away
   by `spring(-lag, last, elev_gain)`.
7. The collision is asked from the point on his axis, not the point beside him.

Sniping, the weapon camera runs with `cameras.sniper`, and its field of view zooms between `fov`
and `zoom_fov` in steps of `1 / tan` of the angle that take `zoom_full_time` for the whole way.

### The collision

A ray from the middle of his body up to 1 above the point looked at lowers the point (and the eye,
and the sponge) to 1 under a ceiling. Then a sweep from the point to the eye stops at the nearest
blocking hit, and a corner held from the last update swings the camera round it rather than close
in. `docs/camera.md` "Collision and corners" has the full reading; parts of it are stand-ins in the
reference (the corner's edge is made up from a hit's point and normal). [assumed]

### Shakes

`cameras.shakes` are every shake of the game by name (angles in degrees). A shake started at a
place weakens with the camera's distance from it; one with none does not. Each update of a shake
aged `t`: [game]

    h = half_duration / duration;  s = t / duration;  k = (h - 1) / h * (h - 0.5)
    strength = clamp((k * s - (k + 1)) * s + 1, 0, 1)
    with a place: strength *= clamp(1 - (distance - full_dist) / (max_dist - full_dist), 0, 1)

Five values, in this order, each limited to `limit = its maximum * strength`: up and down
(`ang_vert`), the tilt (`tilt`), to the side (`ang_horiz`), then the offsets (`pos_horiz`,
`pos_vert`). For each a number is drawn evenly in plus or minus the limit, and the value chases it:

    change = ((1 - damp) * dt * speed + drawn * gain - value) / dt - speed
    speed += clamp(change, -max_acc * dt, max_acc * dt)      (no limit when max_acc is 0)
    value = clamp(value + speed * dt, -limit, limit)

with the angles' `ang_gain`, `ang_damp`, `max_ang_acc` and the offsets' `offs_gain`, `offs_damp`,
`max_offs_acc`. It ends when `t` passes `duration`. All running shakes add up: their turns (the tilt
about the view's line, then up and down, then to the side) multiplied oldest first, their offsets
added along the camera's right and up. The random numbers are the C library's (`seed * 0x343fd +
0x269ec3`) mixed with the clock's seconds, so their sequence cannot be repeated; only their spread
can be compared. [game]

### The pad's rumble

One effect at a time; a new one replaces it. From a kind, a duration and a strength (0 to 1): kind
1 runs the heavy motor at a quarter of the strength fading to nothing over the duration, and the
light one at all of it fading over half; kinds 2 and 3 the heavy motor alone, whole for the
duration then off; kind 0 stops both. [game] A rumble with a place (a blast's `rumble`) with
`scale_type` 1 is whole within `distance_near` of the player, falls evenly to nothing at
`distance_far`, and is not felt past it; with `scale_type` 0 it is the same everywhere.
`cameras.rumbles` are the presets by number: 2 and 4 his melee hit landing (the strong one for the
charged hit and the slam), 3 and 5 his being hit, 8 to 10 climbing, 11 to 13 the car's knocks, 14
the turbo. Presets 0, 1 and 6 are never played. [game]

### The turbo's screen blur

The car's turbo turns on the level's `Environment_SpeedBlur` depth-of-field preset, blended in and
out by its own times. It belongs to the level, not the robot, and is not exported;
`core/src/speedblur.rs` and `docs/shaders.md` have it.

## Sound

Reference: `core/src/sound.rs` (the event player, `Gearbox`, `tyre_sounds`), `core/src/chatter.rs`,
and the Bevy game's `game/src/sound.rs` ([rotf-robots](https://github.com/HUNK2/rotf-robots)), which
decides when each sound plays; `docs/sound.md`. The events and samples are `sounds`, the names
`character_sounds`, the numbers `sound`. The gearbox replays recorded updates exactly [trace];
nothing else has been heard against the original.

### Playing an event

An event of `sounds.events` is FMOD Designer's, and a host with FMOD can rebuild it as it is. For
one without: [game] [data]

- At the start the event's volume is `volume * (1 - volume_random * u)` and its pitch the event's
  with its random part; each sound of each layer draws its own gain, `floor + (1 - floor) * u` times
  the definition's volume (`floor` is `definition_volume_floor`), and its own pitch. `u` is a fresh
  number in 0..1 each time.
- More than `max_playbacks` copies: the oldest playing one is stopped.
- Parameters start at their `min`. A value the code sets is clamped to the range and glides there
  at `seek_speed` shares of the range a second (at once when it has none, or before the event is
  first heard). A parameter with a `velocity` runs by itself at that many units a second and stops
  at its next `sustain` point until the event is keyed off, which lets it run on past one point (a
  loop's tail). `(distance)` is the listener's distance.
- A sound plays while its layer's parameter, as a share of its range, is inside `start` to `start +
  length`; once started, a `one_shot` sound plays to its end. Its volume is the event's times the
  sound's `volume`, its gain, and the layer's `volume` envelopes at their parameters; `pitch`
  envelopes likewise. The sample is picked by `play_mode`. `fine_tune` is in hertz: `pitch = (rate *
  pitch + fine_tune) / rate`.
- Fades: linear over `fade_in_ms` from the start and over `fade_out_ms` from a stop.
- The event's category volumes multiply in, and the game's mix presets (below).

Placed in the world (`positional`): the listener is the camera (its place, forward and up). The
left and right levels follow the source's direction: `sqrt(1 - t)` and `sqrt(t)` at a share `t` of
the way between the two speakers (ahead or behind is 0.707 each). The distance, by the event's
`rolloff`: `custom` (with no curve of its own) leaves it to the event's `(distance)` envelopes;
`linear` multiplies by `(max - d) / (max - min)` and `logarithmic` by `min / d`, with `d` held
between `min_distance` and `max_distance`. Doppler, with the system's scale 0.3 times the event's
level, the sources standing still and the listener moving as the player does (his change of place
over the update; 0 after a jump of over 1000 m/s²):

    pitch *= (340 - (d_now - d_before) / t * scale) / 340,   t = 0.032

The player's own vehicle and flight loops play two-dimensional (their `2dPanLevel` is 1).

### What plays when

- Clip events (`clips[].events`): `sound_play` and `sound_stop`, as FORMAT.md says.
- Steps: a `footstep` or `climb_step` event probes the ground at its node: a ray from 1.5 above the
  foot's ground (the node less the `footstep`'s `height` in `effects.on_models`) to 1.5 below;
  climbing, 1.5 either side of the limb along his facing. The surface hit picks the preset of
  `character_sounds.by_surface`; the sound plays one update later, at the hit. [game]
- His melee hits: each hit of his body plays the `melee_hit` preset for the surface struck, at the
  hit, with his speed as its `speed`. Explosions' hits do not. [game]
- A Trigger to a name also starts the `sound` and `spawned_sound` things of `effects.on_models` with
  that name; an Untrigger keys off a player with `key_off_on_untrigger`, else stops it with its fade.
  A `once` player plays once; one without `retrigger` is not restarted while it plays.
- The car: `engine` from entering the car to leaving it, its `rpm` and `load` from the gearbox
  (below). The tyres' loops, each picked by the ground's surface from `tyre_road` and `tyre_skid`,
  their `speed` the car's: the rolling loop while a wheel is down on a surface and the car is over 1
  m/s, the skid loop with it while skidding (the slide held over 1 m/s, the brake over half and not
  reversing, or the stick at 0.9 or more at 0.85 of the speed the trigger allows). Each restarts when
  the surface changes. `suspension` when a rear spring shortened by more than 0.05 of its length in
  the update. `turbo` when a turbo starts, `turbo_ready` when its cooldown ends. [game]
- Flying: `drive` in the fly mode, `hover` in the hover mode, each from the mode's start to its end;
  its `speed` is his speed, and in flight `pitch` is his pitch over `fly.max_pitch` (over
  `fly.min_pitch` nose down, so never under 0) and `roll` his roll over `fly.max_roll_quick`. The
  flight turbo plays `turbo`. [game]
- Health: `UI_REGEN_START` when his health starts coming back.

### The gearbox

The engine's `rpm` and `load` do not come from the speed. With `sound.gearbox` and `sound.gears`:
[game] [trace]

    up = engine_torque * 2 / (front_wheel_radius * mass);   down = 3.5 * up
    rpm = throttle * revs * gear.ratio * 600         (the throttle counts as full in turbo)

- `revs` (at least 1) climbs by `up / sqrt(revs) * dt` each update the speed is not falling (by more
  than 0.01), and falls by `down / sqrt(revs) * dt / 2` while it is (not in first gear, not sliding).
- Over the gear's `max_rpm` it shifts up and the rpm starts at the next gear's `min_rpm` (in the air
  or sliding it is held at the top instead; past the last gear the revs fall back to where the gear
  before it ran out). Under `min_rpm` it shifts down, at the lower gear's `max_rpm`. In the air the
  rpm is the gear's top.
- Braking (not in turbo, not sliding, not reversing): `revs = down / sqrt(revs) * dt`, the rpm at
  least first gear's `min_rpm`, the load 0.25. Stopped by the brake: first gear, its `min_rpm`, load
  0.5.
- At full throttle and not sliding the speed it looks at is held under `max_speed - |stick| *
  turning_velocity_delta`, the stick eased toward the pad's by `dt` a second.
- The load:

      moved = rpm / rpm_before - 1 + load;   l = clamp(moved, 0, 1)
      load = (0.5 - l) * dt * 4 + l

### The battle chatter

`sound.chatter_lines` are his voice lines, `sound.chatter` the game's `BattleChatter` tuning, and
each line's sample is `sounds.voices` (played through `sound.voice_event`, the event the game uses
for them). The player's character speaks, checked each update, out of the vehicle form and while
no line of his is playing: on dying (once), when his health falls under `charLowHealthPct` of full
(once until it rises again), firing in weapon mode at a target, or in the special. A request picks
one of the lines of that action, category and subcategory evenly, plays it only if a fresh number in
0..1 is at most its `frequency`, after a delay drawn in `minSpeakDelay`..`maxSpeakDelay`; the same
action, category and subcategory are not said again for the action's `*HistoryTime`. [game] What
counts as "at a target" is the AI controller's target in the game; the reference uses the aim
assist's. [assumed] Six of Bumblebee's 115 lines have no sample in the bank.

### The mix

The game's audio presets change category volumes and pitches over time while they run (Optimus's
special runs `OptimusSpecial`). `core/src/audio_mix.rs` has them; the presets are in the event
files. The reverb belongs to the level. [game]

## Effects

Reference: `core/src/particles.rs` (`Effect`, `Group`), `core/src/fxstate.rs`,
`core/src/skidmark.rs`, `core/src/ribbon.rs`; `docs/particles.md`. The effects are `effects` of
`<pack>.json`: `effects.templates` is every particle effect of his pack as the game stores it, with
the picture each element draws, and `effects.on_models` the things on his models that a name or a
clip starts. [game] None of it has been compared with the running game; the pictures were compared
by eye with screenshots.

### What starts an effect

- A clip's `effect_start` event starts its template at its node, until its `effect_stop` or, with
  `auto_stop`, the clip's end.
- A Trigger to a name starts every `effect` of `on_models` with that `trigger`, at its `node`:
  upright when `upright`, following the node while it lives when `follows_node`. Its proxy `mesh`,
  when it has one, is deleted after `timeout.delete` seconds. An Untrigger (`9caa723d`) stops it.
- An `emitter` runs from the model's start when `auto_start`.
- The robot's own state keeps some names on, each sent a Trigger when it turns on and an Untrigger
  when it turns off: [game]
  - in the car: `FX_smoke_exhaust`, the headlights (`Headlight_L`, `Headlight_R`, where the level
    allows them), and `FX_turbocharged` while a turbo runs;
  - flying: `FX_brake` while braking, else `FX_smoke_exhaust` once the take-off clip has sent it;
    `FX_turbocharged` in turbo; hovering: `FX_hover`;
  - the damage smoke by health share: `FX_Damage_1` under 3/4, `FX_Damage_2` also under 1/2,
    `FX_Damage_3` also under 1/4;
  - `FX_regenerate` while health comes back; `FX_stun` while stunned or with the car or the change
    of form disabled; `FX_frost` while slowed.
- A surface's preset (`character_sounds.by_surface`) has an `effect` beside its sound: a step, a melee
  hit, the tyres. Weapons name their own (`impactEffect`, `chargeId`... in `weapons[].numbers`).

### An effect, each update

A template has `elements`; each element draws its own group of particles with its `renderer` and
runs its `actions` in order. The effect has one clock, `time`. For each element: [game]

1. No group yet and `start <= time <= end` (and not stopping): the group is made. A group at or past
   `end` is removed with its particles.
2. The sources at the head of `actions` add particles (not while stopping), at most up to
   `maxParticles` in the group.
3. Every other action runs on every particle.
4. Every age goes up by the step; the particles `Life` marked are removed.

An action runs only while `StartTime <= time <= EndTime`: the effect's time, not the particle's.
Particles are made in the effect's frame and put into the world, unless the element is `localspace`
(then they move with it). The detail level is the highest, so counts are not cut.

The sources, by `id`:

- `Brst`: once, `max(1, trunc(NumParticles))`.
- `SrcA`: `EmissionRate * dt + carry` a step; the whole part is made, the fraction carried.
- `Ejct` (a moving emitter: exhausts, dust, trails): as `SrcA`, but nothing in an update in which the
  emitter moved less than `MinSpeedToSpawn` (a distance in one update); what is made is laid evenly
  from the emitter's last place to this one and leaves with `(way moved / dt) * SpeedCoeff`.
- `RSpn`: a clock goes down by the step; each time it is at or under 0 one particle is made and a
  wait drawn from the 17th domain is added (ten at most a step).
- `Intt`: `SrcA` that goes on and off: a clock; when it runs out, a time is drawn from domain 0
  (going on) or domain 1 (going off); particles only while on. [assumed: it starts off, at 0]

A new particle takes from the source's `domains`, in this order (each draw in turn): 0 place, 1
radius, 2 width, 3 colour, 4 alpha, 5 a facing, 6 turn, 7 height, 8 frame, 9 lifetime, 10 most
alpha, 11 velocity, 13, 12 turning rate, 14, 15. A `NULL` domain gives the default: place 0, radius,
width, height, alpha, most alpha and lifetime 1, colour (1, 1, 1), turn 1, frame, turning rate and
velocity 0. (`Ejct` and `Intt` lay their slots out differently: `docs/particles.md`, "Sources".)
The domains, `u` a fresh number in 0..1:

- `1dPt`, `3dPt`: `Val`. `1dBx`, `3dBx`: `Min + (Max - Min) * u`, each axis its own `u`.
- `1dBu`, `3dBu`: the same with `u` bent: `2u(1 - u)` under a half, `2u(u - 1) + 1` over.
- `Crcl`: on the rim of `Radius` round `Center`, across `Normal`, at an even angle.
- `Disc`: inside that circle: two numbers in plus or minus `Radius`, drawn again while outside (100
  tries).
- `Sphe`: x and y in plus or minus `Radius`, z in 0..`Radius` (the upper half of a ball), drawn again
  while outside (30 tries), plus `Center`.
- `SpSu`: three numbers in -1..1 made a unit vector, times `Radius`, plus `Center`.
- `Line`: `Min + (Max - Min) * u`, one `u` for all three.

The actions, with `f = age / lifetime`:

- `Move`: place += velocity * dt; turn += turning rate * dt.
- `Life`: removed when lifetime < age (the age before this update's).
- `LnSc`, `LScW`, `LScH`: radius, width, height += `DeltaRadius`, `DeltaWidth`, `DeltaHeight` * dt.
- `ExSc`: radius *= `1 + x + x²/2`, `x = ln(Exponent) * dt`.
- `VlDr`, `AVDr`: velocity, or turning rate, *= `1 + x + x²/2 + x³/6`, `x = -Exponent * dt`.
- `Forc`: velocity += (`AccX`, `AccY`, `AccZ`) * dt, turned by the group's frame with `LocalSpace`.
- `PAtr`: velocity -= the direction from the centre * `Force` * dt / max(distance², 0.001).
- `AlFd`: alpha = `BeginAlpha` while `f < FadeBegin`; then `(1 - f) * BeginAlpha / (1 - FadeBegin)`;
  0 from the lifetime on.
- `AFIO`: `f * MaxAlpha / EndFadeIn` while under `EndFadeIn`, `MaxAlpha` until `BeginFadeOut`, then
  `(1 - f) * MaxAlpha / (1 - BeginFadeOut)`, 0 past 1. `RFIO`: the same with the particle's own most
  alpha.
- `ClSh`: between `BeginShiftTime` and `EndShiftTime` (shares of the lifetime) the colour goes evenly
  from its begin colour to its end colour; outside that it is left alone.
- `UVFr`: frame = max(frame, `BeginFrame`) + step, wrapped inside `BeginFrame`..`EndFrame`; the step
  is `FPS * dt`, or with `FrameCycles` over 0, `range * FrameCycles * dt / lifetime`.
- `AngT`: the picture lies along the velocity on the screen.

Not read, and so doing nothing in the reference: the `modifiers` lists and the actions no robot's
pack was seen to use (`Traj`, `SpAc`, `SpDe`, `PsMv`, `OMov` and others).

### Drawing a particle

`effects.templates[].textures` is each element's picture. The renderer's `id` is its kind: the
rectangles (`Rect`, `CRct`, `UVRe`, `cUVr`) have half-sizes `width` and `height`; `Dstr` bends the
picture behind instead of drawing; every other kind is a square of half-size `radius`. A quad faces
the camera, turned anticlockwise by `turn * 2 pi`; `VelocityTracked` lays it along the velocity
instead. `hAlign` and `vAlign` 1 or 2 move it to one side of its place (`vAlign` 2 stands it on
its place). [game]

- A sheet of `WidthFrames` by `HeightFrames` pictures: the frame's whole part picks the picture,
  across then down; with `UVBlending` the next one is mixed in by the fraction.
- Colour: the texel times the particle's colour and alpha times `Tint / 255`; the alpha also times
  `saturate(depth * a + b)`, `a = 1 / (AlphaFadeEnd - AlphaFadeStart)` and `b = -AlphaFadeStart * a`
  (a fade in with the distance in front of the camera; none when the two are equal). A particle with
  alpha under 0.05 is not drawn.
- `BlendMode`: 0 and 3 alpha blending, 1 added, 2 subtracted, 4 written as it is. 3 is also lit:
  `light = saturate(max((1 - |N.L|)(1 - a) + BackLitCoeff, N.L))`, `rgb = texel * colour *
  (DiffuseCoeff * sun * light + AmbientCoeff * ambient + saturate(1 - AmbientCoeff - DiffuseCoeff))`,
  the sun and the ambient being the level's.
- `ZFeatherDistance` over 0: soft where it meets the scene behind.

Not followed: `bloomCoeff`, `DropOffDist`, the `nanoGraph`, the sort.

### Skid marks and ribbons

- A `skid_mark` sits on each tyre. While the tyres skid on a surface above 1 m/s it lays a point at
  the tyre, 0.1 above the contact (the first 0.01), each time the tyre is more than `point_length`
  from the last; at most 50 points, the oldest reused. A quad joins each point to the one before,
  `point_width` to each side of the way, coloured `colour`, with no alpha at a strip's first and last
  points. Leaving the ground or stopping breaks the strip. [game]
- A `ribbon` is a strip drawn through its `links` (nodes) as they move: a row of their places sampled
  every 1/60 s while it is on (and, with `trigger_speed` over 0, only while a node moved that far),
  `trails` rows long, drawn as a smooth curve of `subdivisions` steps between rows. A Trigger turns it
  on, an Untrigger off; `enabled` is how it starts. [game] `core/src/ribbon.rs` has the curve.

Not exported: the lights (the muzzle flash's) and the car's knock effects (`CollisionFX`);
`core/src/point_light.rs` and `collisionfx.rs` have them.

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
