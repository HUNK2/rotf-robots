//! The deployable turret (Ironhide's and Blackout's special): the thrown body, its landing,
//! the cap on how many stand at once, the turret's rotator, its fire timer and its target
//! choice. Read from the game's code (docs/special-weapons.md, "Turret code (second read)").
//! No engine types; space is the game's (x right, y forward, z up, metres), time in seconds.
//! Not wired into sim.rs or the game yet.
//!
//!   [game addr]  read in the game's code at that address
//!   [data]       a value from the game's data, used as its name says
//!   [assumed]    our reading of what a value or a step is for
//!   [stand-in]   invented so the turret can be used; to be replaced
//!   [unknown]    not found in the code read; left out or kept as a field only

use std::f32::consts::{PI, TAU};

use glam::Vec3;

use crate::formats::lxb::Node;

/// Degrees to radians, the literal the loader multiplies by [game 008139b0].
const DEG: f32 = 0.017453292;

/// The body's linear damping per physics step [data blackout_turret `DynamicsData` linearDamping
/// 0.9995; game 005b9970 stores it at body `+0x2c`, 006e96c0 multiplies the velocity by it].
pub const LINEAR_DAMPING: f32 = 0.9995;

/// The fire turret's band, from its FireTurret loader 0079e250 [game]: `minFireRange` (ft+0x60),
/// `maxFireRange` (ft+0x64), `maxFireAngle` (ft+0x68, yaw, radians) and `maxFireElevation`
/// (ft+0x6c, pitch, radians). Blackout's data: 0, 300, 360 and 360 degrees [data
/// work/survey/weapons/blackout.txt l.720724-720728].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FireBand {
    pub min_range: f32,
    pub max_range: f32,
    pub max_yaw: f32,
    pub max_pitch: f32,
}

/// A candidate of the fire turret's query: (id, team, the point aimed at, present and not hidden,
/// radius). The radius is the candidate's own (`EDI+0x14` in 007996c0); the caller takes it from
/// the scout's tuning [assumed: its `robot_radius`].
pub type Candidate = (u32, u32, Vec3, bool, f32);

/// The TurretLauncher's numbers, read from the weapon's attribute block (the block that the
/// launch reads as `wpn+0x130..0x13c`, loaded by 007bfa30) [game 0078c2e0, 0078b0a0].
#[derive(Clone, Debug, PartialEq)]
pub struct LauncherTuning {
    /// `launchSpeed` (data 50): the throw speed along the dummy direction, m/s [game 0078c2e0 d] [data].
    pub launch_speed: f32,
    /// `zGravity` (data -64): z acceleration of the thrown body, m/s^2 [game 0078c2e0 e] [data].
    pub z_gravity: f32,
    /// `lifeTime` (data -1): above zero the launch starts a timer; -1 starts none [game 0078c2e0 f] [data].
    pub life_time: f32,
    /// `maxTurretsAtOnce` (data 1): the cap enforced by 0078b0a0 [game] [data].
    pub max_turrets_at_once: u32,
    /// Fields the data did not have (read as 0).
    pub missing: Vec<String>,
}

/// Reads the launcher numbers from an attribute block by field name (the same helpers as
/// weapons.rs `tuning_from`).
pub fn read_launcher(node: Node) -> LauncherTuning {
    let mut missing = Vec::new();
    // [game 007bf9b0] what the constructor leaves when the data has no value.
    let default = |name: &str| match name {
        "launchSpeed" => Some(40.0),
        "zGravity" => Some(-32.0),
        "lifeTime" => Some(-1.0),
        "maxTurretsAtOnce" => Some(1.0),
        _ => None,
    };
    let mut num = |name: &str| match node.get(name).and_then(Node::float) {
        Some(v) => v,
        None => default(name).unwrap_or_else(|| {
            missing.push(name.to_owned());
            0.0
        }),
    };
    let launch_speed = num("launchSpeed");
    let z_gravity = num("zGravity");
    let life_time = num("lifeTime");
    let max_turrets_at_once = num("maxTurretsAtOnce").max(0.0) as u32;
    LauncherTuning {
        launch_speed,
        z_gravity,
        life_time,
        max_turrets_at_once,
        missing,
    }
}

/// A deployed turret's aiming parts as the data gives them: its `FireTurret` (loader 0079e250:
/// the band, `coneAngleToAllowFire`, `fireRate`) and its `TurretRotator` (008139b0: the rates and
/// accelerations, and the yaw and pitch limits), all in degrees there. [game] [data] Blackout's:
/// fireRate 100, band 0..300 m and 360 / 360 degrees, cone 15, rotator 70 / 50 deg/s; Ironhide's:
/// fireRate 25, band 0..300 m and 180 / 180, cone 30, rotator 360 / 180 deg/s with 1080 accelerations
/// and pitch -90..90 [data work/survey/weapons/ironhide.txt l.683917-683950].
#[derive(Clone, Debug, PartialEq)]
pub struct Aiming {
    pub band: FireBand,
    pub cone_deg: f32,
    pub fire_rate: f32,
    pub rotator: Rotator,
}

/// The `FireTurret` and `TurretRotator` among a deployed turret object's attachments (the
/// `deployableTurretObj`'s `obj`); None when it lacks either.
pub fn read_aiming(object: Node) -> Option<Aiming> {
    let things: Vec<Node> = object.get("attachments").into_iter().flat_map(|a| a.items()).filter_map(|a| a.get("thing")).collect();
    let of = |class: &str| things.iter().find(|t| t.type_name() == Some(class)).copied();
    let (fire, turn) = (of("FireTurret")?, of("TurretRotator")?);
    let num = |node: Node, name: &str, default: f32| node.get(name).and_then(Node::float).unwrap_or(default);
    let band = FireBand {
        min_range: num(fire, "minFireRange", 0.0),
        max_range: num(fire, "maxFireRange", 0.0),
        max_yaw: num(fire, "maxFireAngle", 0.0) * DEG,
        max_pitch: num(fire, "maxFireElevation", 0.0) * DEG,
    };
    let mut rotator = Rotator::from_data_degrees(
        [num(turn, "maxTurnRateLeftRight", 0.0), num(turn, "maxTurnRateUpDown", 0.0)],
        [num(turn, "turnAccelerationLeftRight", 0.0), num(turn, "turnAccelerationUpDown", 0.0)],
    );
    rotator.min_yaw = num(turn, "minYawAngle", -180.0) * DEG;
    rotator.max_yaw = num(turn, "maxYawAngle", 180.0) * DEG;
    rotator.min_pitch = num(turn, "minPitchAngle", -180.0) * DEG;
    rotator.max_pitch = num(turn, "maxPitchAngle", 180.0) * DEG;
    Some(Aiming { band, cone_deg: num(fire, "coneAngleToAllowFire", 0.0), fire_rate: num(fire, "fireRate", 0.0), rotator })
}

/// The gravity scale of the thrown body. [game 0078c2e0 l.59-71] The game stores `zGravity` times
/// `body+0x24` in the body's gravity (`body+0x18`) when `body+0x28` is set. [game 006e8e20] says
/// `+0x24` is the body's mass and `+0x28` its inverse mass; the force then integrates with the
/// inverse mass [game 006de230: `v += invmass dt force`] so the net acceleration is `zGravity`.
/// The scale in this port is therefore 1 for the acceleration, and the velocity below is the
/// acceleration [game 006e96c0 adds the gravity to the force].
pub const BODY_GRAVITY_SCALE: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Thrown, under gravity, not yet on the ground [game 0078b3b0: runs while rec+0x4c == 0].
    Flying,
    /// On the ground, the landing clip `animOnLand` plays and the fire gate is still shut.
    /// [game 0078b8e0 l.109-136] The landing sets the DEPLOYABLE's own state (`rec+0x4c = 1`)
    /// and plays the clip; it does not open the fire gate.
    /// Read 2026-10-20, the fire gate is a different object: `ft` (the FireTurret, class
    /// c8990671, vtable 00b5eed4, found by `fire_turret_getter` 00815550) is NOT `rec` (the
    /// DeployableTurret, class ab348ec5, vtable 00b5ebf8, from 00745f10). The gate `ft+0x4c`
    /// is read by 0079e670 and 0079f0f0 and written by the FireTurret's Enable slot 0079e4d0
    /// (`ft+0x4c = arg`). Slot 6, the Trigger handler 00815130, pushes 1 and calls that Enable
    /// [game 00815130, 0079e4d0; the dispatch 0080df70 sends Trigger to vslot 6]. In the Blackout
    /// data the clip's Trigger (`d5d636c1`) to the name `Turret` (-833717741, the FireTurret's
    /// LuxScriptName) is at tick 90 of the animOnLand clip, the deploy time [data
    /// work/survey/weapons/blackout.txt l.707991-707992]. So the game's gate opens at the END of
    /// the landing clip, not at the landing: Deploying (gate shut) and Active (gate open) both
    /// match the game for Blackout. Merging them would drop the gate, so they stay.
    /// Ironhide's turret has the same gate: its thing list carries a FireTurret and a TurretRotator
    /// named `Turret` (-833717741) after its TargetLockable [data work/survey/weapons/ironhide.txt
    /// l.683917-683950; an earlier read stopped short of them]. His animOnLand clip (animID
    /// -1785250356, l.665068) sends the same Trigger (`d5d636c1`) at tick 86 to that name, so his
    /// gate opens at tick 86 and his FireTurret fires his turret's MissileLauncher. [data; seen
    /// firing in the game, as Grindor's, 2026-10-27] The Trigger also reaches the TargetLockable
    /// (vslot 6 = 007bb230 `targetlockable_enable`) [game], whose `startEnabled` is already true.
    Deploying,
    /// Deployed and the fire gate open: `ft+0x4c` set by the clip's Trigger [game 00815130,
    /// 0079e4d0; data, blackout.txt l.707991-707992] (Blackout). [stand-in] The game's removals are the cap [game 0078b0a0] and the
    /// owner tests of the update [game 0078cd50]: the owner's first word null, or the owner's
    /// `+0x148` bit 3 set. The port holds no owner handle: `game/src/player.rs` passes the player's `alive` (`health > 0`, `TurretOwner`) as the owner test, a proxy for the dead flag, and `tick_turrets` removes the turrets when it is false (`game/src/weapons.rs` l.1113-1118); the cap counts every turret of that one owner.
    /// settles: the owner handle the port would need (its character at `rec+0x38`, read by
    /// 0078cd50 l.1-20 through `character_object_lookup` 0070a3c0) and a call from the caller
    /// (game/src/weapons.rs) that passes the owner's dead flag into the turret's step; the
    /// turret's removal by lifeTime is `turret_message` 0078c2b0 -> 0078cc10 (the port has it).
    /// Read 2026-10-20: 0078cd50 (owner tests, dead flag bit 3 = `DEAD`, see `damage.rs`),
    /// 0078b0a0 (cap by owner, smallest `+0x48`), 0078c2b0 (lifeTime and ddf4c407 messages).
    Active,
    /// Removed by the cap (rec+0x4c == 2 in the game) [game 0078cc10].
    Removed,
}

/// One thrown or deployed turret: the physics body and the state.
#[derive(Clone, Debug, PartialEq)]
pub struct Turret {
    pub position: Vec3,
    pub velocity: Vec3,
    /// z acceleration of the body (`zGravity` times `BODY_GRAVITY_SCALE`), m/s^2.
    pub gravity_z: f32,
    pub state: State,
    /// When it was launched, in seconds of game time. The cap removes the smallest [game 0078b0a0]
    /// (`rec+0x48` is `global_game_state()+0xc` at the spawn [game 0078b2e0]; `+0xc` is the game
    /// time [game 005a9cc0, damage.md "time +0x38"]).
    pub spawned_at: f32,
    /// How long the Deploying state lasts. [data] given by the caller: the game passes the
    /// `animOnLand` clip's ticks over 60 (Blackout 90, Ironhide 86) [game/src/weapons.rs 804,
    /// 824]; the clip's on-done chain is [game 0078b8e0 l.122-131]. No default is invented.
    /// The game has no timer for it. On landing it plays `animOnLand` (`rec+0x2c`) with the
    /// idle anim (`rec+0x30`) as the clip's on-done [game 0078b8e0 l.122-131], and the record
    /// stays in state 1 [game 0078b8e0 l.136]: the end of the landing clip chains to the idle
    /// clip and changes no state. So this time is the length of `animOnLand` [settles: that
    /// clip's length, in the Blackout / Ironhide clip data].
    pub deploy_time: f32,
    deploy_left: f32,
    /// `lifeTime` countdown when `lifeTime` is above zero [game 0078c2e0 f]. Its effect on the
    /// turret is [unknown] (no removal by it was found), so it only counts down.
    pub life_left: Option<f32>,
    /// The rest facing: the turret's base +y in world space at the launch [game 0078c2e0 l.15-31].
    /// The frame is align_z (0056e7b0) of the source's +y row, which is the throw direction, with
    /// the normal (0,0,1); the align projects that row onto the ground plane, and then Rz(pi), so
    /// the base faces back along the throw: `-(throw direction, flattened)` [game]. The projection
    /// is the same as this flattening. When the throw is steeper than |dot| > 0.96 [game 0056e7b0
    /// l.9-10] the align takes a cross product with the source's x row instead. The rule in full
    /// is `rest_facing_for_frame` [game]; `launch` has only the +y row, so for a steep throw it
    /// keeps the flattened rule [stand-in] (and +y when the throw is vertical). settles: the caller
    /// (game/src/weapons.rs `launch_turret`) passing the launcher dummy's x row to
    /// `rest_facing_for_frame`.
    pub rest_facing: Vec3,
    /// The holder's up row: world z while flying, the contact normal once it has landed [game
    /// 0078b8e0 l.95-103 with the flag 1 from 0078b72f]. Nothing in the port reads it yet (the
    /// turret's aim frame is not ported), so it is data only.
    pub up: Vec3,
}

impl Turret {
    /// Seconds since it landed while it deploys (the `Deploy` clip's time), the whole deploy time
    /// once Active, 0 in flight.
    pub fn deployed_for(&self) -> f32 {
        match self.state {
            State::Flying => 0.0,
            State::Deploying => (self.deploy_time - self.deploy_left).max(0.0),
            State::Active | State::Removed => self.deploy_time,
        }
    }
}

impl LauncherTuning {
    /// The throw [game 0078c2e0 a-g]. `direction` is the launcher's dummy direction in world
    /// space; it is normalised, and zero when its length is 1e-8 or less [game 0078c2e0 b].
    /// The velocity is `launch_speed * direction` [game 0078c2e0 l.50-54]; gravity is `z_gravity`.
    /// The game gives the body gravity only when `body+0x28` (its inverse mass) is set [game
    /// 0078c2e0 l.59]; the thrown body has a mass in its data (Blackout's `blackout_turret`
    /// Collision `mass` 13669.3 [data]). Ironhide's template's mass was not read.
    /// The 180-degree turn of the spawn frame [game 0078c2e0 c] is a frame, not the velocity,
    /// and is not applied here. `now` is the game time of the launch; `deploy_time` is the
    /// Deploying time (see `Turret::deploy_time`).
    pub fn launch(&self, muzzle: Vec3, direction: Vec3, now: f32, deploy_time: f32) -> Turret {
        let dir = if direction.length() > 1e-8 {
            direction.normalize()
        } else {
            Vec3::ZERO
        };
        let flat = Vec3::new(dir.x, dir.y, 0.0);
        let rest_facing = if flat.length() > 1e-8 {
            -flat.normalize()
        } else {
            Vec3::Y
        };
        Turret {
            rest_facing,
            up: Vec3::Z,
            position: muzzle,
            velocity: dir * self.launch_speed,
            gravity_z: self.z_gravity * BODY_GRAVITY_SCALE,
            state: State::Flying,
            spawned_at: now,
            deploy_time,
            deploy_left: 0.0,
            // A timer only when lifeTime > 0 [game 0078c2e0 f].
            life_left: if self.life_time > 0.0 { Some(self.life_time) } else { None },
        }
    }
}

/// The rest facing of a thrown turret from the launcher's frame, the game's rule in full
/// [game 0078c2e0 l.15-31 with 0056e7b0 align_z_matrix]. The spawn frame is align_z with the
/// normal (0,0,1): the source's +y row `y_row` is projected onto the ground plane when
/// `|y_row . z| <= 0.96`; a steeper throw takes the cross product of the normal with the source's
/// +x row `x_row` instead [0056e7b0 l.9-10, 14-22]. Then Rz(pi) turns the base, so the base's
/// +y (the rest facing) is the negated flat direction. Zero in the cross case with a zero
/// `x_row`: +y, as launch() does. [game]
pub fn rest_facing_for_frame(y_row: Vec3, x_row: Vec3) -> Vec3 {
    let n = Vec3::Z;
    let d = y_row.dot(n);
    let l = if d.abs() <= 0.96 { y_row - n * d } else { n.cross(x_row) };
    let l = if l.length() > 1e-8 { l.normalize() } else { Vec3::ZERO };
    let rest = Vec3::new(-l.x, -l.y, 0.0);
    if rest.length() > 1e-8 {
        rest.normalize()
    } else {
        Vec3::Y
    }
}

impl Turret {
    /// One step of the body. `contact` is the ground contact this step, as its normal (the
    /// caller's contact test); `None` when there is none.
    ///
    /// Flying: a contact whose normal, normalised, has z > 0.5 lands the turret [game 0078b3b0
    /// l.31-54: `dot(normal, up) > 0.5`, the gate of the response 0078b8e0]. The up vector is
    /// world z here while flying: the game's holder up row `rec+0xc +0x28..0x30` is world z at the
    /// launch [game 0078c2e0: align_z of (0,0,1), then Rz(pi)], and nothing in flight writes it
    /// [game 0078b3b0 l.1-54: the only writer is the landing response].
    /// The landing [game 0078b72f-0078b740 -> 0078b8e0, 4th argument 1]: the response writes the
    /// frame rows to `rec+0xc +0x10..0x30`, and with that flag the up row (`+0x28..0x30`) is the
    /// contact normal itself, normalised (0078b8e0 l.70-80, 95-103). So the port's `Turret::up`
    /// is set to the normal at the landing. Its other rows (the cross products, from the buffer
    /// `esp+0x9c` that 0078b3b0 fills by the normalised copy at 0078b6d2) are not kept.
    /// The 'dot > 0.5' test uses the up row before the landing, which is world z in the port.
    /// [game 0078b3b0 l.31 gate `uVar2 & uVar1 & 1`, not ported: OPEN, see below]
    /// settles (for the gate only): the two u16 flags at body `+0x9c` and the other body's `+0x9e`
    /// (0078b3b0 l.7-9, 11-22: `uVar3` is the test; the flags are written by the contact setup,
    /// not read here).
    /// A contact at or under that gate does nothing in the game [game 0078b3b0: no branch for
    /// it] (the contact-flag gate `uVar2 & uVar1 & 1` is not ported [assumed]). Bit 1 is `PUSH`
    /// (docs/melee.md mask list). blackout_turret's primitives carry maskPreset `DynamicObj` (48ca5531)
    /// with sendMask 175 (0xaf, PUSH among them) and receiveMask 1 (PUSH) [data, work\survey\weapons\
    /// blackout.txt l.1006-1008]. The presets, decoded (wave 28 P1, global_collision.txt): DynamicObj
    /// (1221219633) sends PUSH|PUNCH|AVATAR_PUSH|CAMERA|WATER|DETECT and receives PUSH; StaticGround
    /// (-1983227808) sends PUSH|PUNCH|AVATAR_PUSH|GROUND|CAMERA|DETECT and receives nothing. The gate is
    /// B.send & A.recv & PUSH (B = param_2[1], A = *param_2), so a turret on static ground passes only
    /// as A [data, game]. [assumed: the turret is A] settles: the pair builder that fills 0078b3b0's
    /// param_2 (its caller is not found by `fn.py callers`; the contact dispatch in 006e2c10).
    /// The game's landing response zeroes the velocity when `in_EAX[0]` is nonzero [game 0078b8e0
    /// l.116-120]. The caller passes `in_EAX` as the stack buffer at `esp+0x34`, which holds the
    /// normal's z component [game 0078b3b0 l.63, raw disasm 0078b560-0078b5c0: read with care,
    /// the buffer layout is from a partly aligned decode]. A landing has z > 0.5, so the test
    /// passes and the port's zeroing matches [game].
    /// With no landing the body moves as the game's step does [game 006df7c0: 006e96c0 (damping
    /// and the gravity force), 006de230 (the force into the velocity, `v += g*dt`), then the
    /// position from the new velocity (006ddad0), `p += v*dt`]. The damping comes first, as
    /// 006e96c0 does it: `v *= LINEAR_DAMPING`, and the velocity is zeroed when its squared
    /// length is under 2.5e-5 or over 1e5 [game 006e96c0]. The damping is the body's `+0x2c`:
    /// the Collision loader 005b9250 stores the data's `linearDamping` there (`movss [ebx+0x2c]`,
    /// 005b9970; default 0.9995, `00b6ae8c`), and blackout_turret's `DynamicsData` carries
    /// 0.9995 (work/survey/weapons/blackout.txt). Applied once per call, the game's physics
    /// step (006e18a0 calls 006df7c0 once per update) [game 006dedf0 via 00ad6e20, 005a8740 l.7-9].
    /// The wrapper is a World::Update list callback: one call per update, with its dt (007799f0).
    ///
    /// Deploying: counts `deploy_left` down; at zero the state is Active. For Blackout this is the
    /// game's Trigger at tick 90 of the landing clip, which opens the fire gate `ft+0x4c` (see the
    /// State doc) [game 00815130, 0079e4d0; data blackout.txt]. The game's record has no timer of
    /// its own after landing [game 0078b8e0 l.136]. The anims are not played here: the landing
    /// plays `animOnLand` [game 0078b8e0 l.122-131], the port does not.
    pub fn step(&mut self, dt: f32, contact: Option<Vec3>) {
        if let Some(life) = self.life_left.as_mut() {
            *life -= dt;
        }
        match self.state {
            State::Flying => {
                if let Some(normal) = contact {
                    let n = normal.normalize_or_zero();
                    if n.z > 0.5 {
                        self.state = State::Deploying;
                        // [game 0078b8e0 l.70-80, 95-103] the up row becomes the normal.
                        self.up = n;
                        self.velocity = Vec3::ZERO;
                        self.deploy_left = self.deploy_time;
                        return;
                    }
                    // [game 0078b3b0] a steep contact gets no response: the flight carries on.
                }
                // [game 006e96c0] damping first, then the zeroing of a tiny or huge velocity.
                self.velocity *= LINEAR_DAMPING;
                let v2 = self.velocity.length_squared();
                if !(2.5e-5..=1.0e5).contains(&v2) {
                    self.velocity = Vec3::ZERO;
                }
                let g = Vec3::new(0.0, 0.0, self.gravity_z);
                self.velocity += g * dt;
                self.position += self.velocity * dt;
            }
            State::Deploying => {
                self.deploy_left -= dt;
                if self.deploy_left <= 0.0 {
                    self.state = State::Active;
                }
            }
            State::Active | State::Removed => {}
        }
    }
}

/// The cap [game 0078b0a0]: when more turrets stand than `max`, the OLDEST (smallest
/// `spawned_at`) is removed. The game removes one per call and is called once per step
/// (through the vtable), so this removes one per call too. Removed turrets are dropped from
/// the list. Ties keep the first in the list [game 0078b0a0: `+0x48 < best` strictly, so the
/// first of the list's walk keeps it]. The game counts only the same owner's turrets; the port
/// has no owner and counts them all. settles: the owner pair at `rec+0x38` (0078b0a0 l.4-30 counts
/// by it) and the port's owner once a turret has one.
pub fn enforce_cap(turrets: &mut Vec<Turret>, max: u32) {
    if let Some(i) = cap_index(turrets, max) {
        turrets.remove(i);
    }
}

/// The index of the turret the cap removes this call, or `None` when the count is within
/// `max` [game 0078b0a0]. The caller removes it from its own list (kept index for index with
/// any gear the turret has).
pub fn cap_index(turrets: &[Turret], max: u32) -> Option<usize> {
    let live: Vec<usize> = (0..turrets.len())
        .filter(|&i| turrets[i].state != State::Removed)
        .collect();
    if live.is_empty() || (live.len() as u32) <= max {
        return None;
    }
    let mut oldest = live[0];
    for &i in &live[1..] {
        if turrets[i].spawned_at < turrets[oldest].spawned_at {
            oldest = i;
        }
    }
    Some(oldest)
}

/// A turret gun's burst: one allowed shot of the gun lets go `burst_per_fire` rounds,
/// `timeBetweenEachBurst` apart, the first at once [data: Blackout_TurretGatlingAttribute,
/// burstPerFire 3, timeBetweenEachBurst 0.08]. [game 007ae010, 007ac370] The weapon's own update
/// runs the burst by itself: once per game update while rounds are left it adds the update to
/// the burst clock and, when the clock has reached the gap, fires again and takes the gap off
/// the clock (kept at 0 or more); the press's own round comes after that update's step (the
/// same rule as `special_weapon::SpecialFire`). So the gatling's three rounds leave 3 and 2 or
/// 3 updates apart, not every 0.08 s. [game 007ac370 l.38-84] The game's gate for a due round
/// is `isSpecial`, or `isVehicleModeWeapon` while in motion, else the weapons-out byte `+0x38`
/// [game 007a75a0]. This struct does not test the gate, so it is right only for a gun that is
/// `isSpecial`. The turret's gun is not identified (its `rec` class `3d2c29e2` is not found), so
/// the gate is OPEN: settles: the owner of `FUN_0078fc80` (class `3d2c29e2`) and its `isSpecial`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Burst {
    /// Rounds still to go.
    pub left: u32,
    /// Seconds between two rounds.
    pub gap: f32,
    /// The burst clock (weapon word `0xfe`), seconds.
    pub clock: f32,
    /// A started burst whose first round has not left yet.
    pending: bool,
    /// Time not yet made into whole game updates.
    carry: f32,
}

impl Burst {
    /// A new burst of `rounds` rounds, `gap` seconds apart; the first leaves in the next `step`.
    pub fn start(&mut self, rounds: u32, gap: f32) {
        self.left = rounds;
        self.gap = gap.max(0.0);
        self.clock = 0.0;
        self.pending = rounds > 0;
    }

    fn fire(&mut self) -> u32 {
        self.left -= 1;
        self.clock = (self.clock - self.gap).max(0.0);
        1
    }

    /// Advances by `dt` seconds and returns the rounds that leave in this step.
    pub fn step(&mut self, dt: f32) -> u32 {
        let mut out = 0;
        self.carry += dt;
        while self.carry >= crate::sim::GAME_UPDATE {
            self.carry -= crate::sim::GAME_UPDATE;
            if self.left == 0 || self.pending {
                continue;
            }
            self.clock += crate::sim::GAME_UPDATE;
            if self.gap <= self.clock {
                out += self.fire();
            }
        }
        if self.pending {
            self.pending = false;
            out += self.fire();
        }
        out
    }
}

/// Yaw and pitch rotator of a deployed turret (`rot+`) [game 00813e30, 00813470, 008139b0].
/// Angles and rates are radians and radians per second.
#[derive(Clone, Debug, PartialEq)]
pub struct Rotator {
    pub yaw: f32,
    pub pitch: f32,
    pub yaw_rate: f32,
    pub pitch_rate: f32,
    pub min_yaw: f32,
    pub max_yaw: f32,
    pub min_pitch: f32,
    pub max_pitch: f32,
    /// `maxTurnRateLeftRight` (yaw cap), rad/s.
    pub max_rate_yaw: f32,
    /// `turnAccelerationLeftRight`, rad/s^2.
    pub accel_yaw: f32,
    /// `maxTurnRateUpDown` (pitch cap), rad/s.
    pub max_rate_pitch: f32,
    /// `turnAccelerationUpDown`, rad/s^2.
    pub accel_pitch: f32,
    /// The point the turret turns to when it has no target: `rest` along the base +y [game
    /// 00813e30 l.102-106, 008153e0]. Set it to the turret's `rest_facing` at the launch, which
    /// the game's turret path always does (game\src\weapons.rs `launch_turret`). The +y of
    /// `from_data_degrees` is for a Rotator made without a launch (the tests) [port].
    pub rest: Vec3,
}

impl Rotator {
    /// From the data's numbers as the game stores them: the two rates and the two accelerations
    /// in DEGREES (per second, per second squared), `[left-right, up-down]` [game 008139b0: each is
    /// multiplied by 0.017453292]. The limits are -180..180 degrees [data] [game 00813470 default].
    pub fn from_data_degrees(max_turn_rate: [f32; 2], turn_accel: [f32; 2]) -> Self {
        Rotator {
            yaw: 0.0,
            pitch: 0.0,
            yaw_rate: 0.0,
            pitch_rate: 0.0,
            min_yaw: -PI,
            max_yaw: PI,
            min_pitch: -PI,
            max_pitch: PI,
            max_rate_yaw: max_turn_rate[0] * DEG,
            accel_yaw: turn_accel[0] * DEG,
            max_rate_pitch: max_turn_rate[1] * DEG,
            accel_pitch: turn_accel[1] * DEG,
            rest: Vec3::Y,
        }
    }

    /// One step toward `target` (a world point), from the turret's position `from`.
    /// Desired yaw and pitch: `yaw = atan2(-dx, dy)`, `pitch = atan2(-dz, hypot(dx, dy))` with
    /// `d = target - from` [game 00813e30 b]; FUN_0046be80 is `_CIatan2` [game, checked].
    /// With no target the game aims 10 m along the rotator bone's +y in its base pose [game
    /// 00813e30 l.102-106, 008153e0]: a rest facing, not the current angles. The port aims at
    /// `rest` (the launch's rest facing, `Turret::rest_facing`) [stand-in until the caller sets
    /// it]. settles: the base pose of 008153e0 is the object's frame at launch (not checked
    /// against a recording of the turn back).
    ///
    /// One step toward `target` (a world point), from the turret's position `from`.
    /// Checked against the disassembly of 00813e30 (2026-10-12; see docs/special-weapons.md
    /// "TurretRotator update, checked"). The steps, with `dt` the game step:
    /// 1. [game 00813e30 b] Desired angles: `want_yaw = atan2(-dx, dy)`,
    ///    `want_pitch = atan2(-dz, hypot(dx, dy))`, `d = target - from`.
    /// 2. [game 00813e30 0x813f..] Caps: if `max_rate_yaw <= 0` or `max_rate_pitch <= 0`, the
    ///    angles snap to the desired ones and the rates are left as they are [game 00813e30
    ///    0x8143c3..0x8143d4: the gate of the rate block].
    /// 3. [game 00813e30 0x8143d4, 0x8143e8 path, 0x814...] Errors: `err = wrap(want - angle)`
    ///    for both axes, wrapped to [-pi, pi) by FUN_0056f290 (floor-based) [game 00813e30 line
    ///    131-137, 0x814420, 0x81447e].
    /// 4. [game 00813e30 0x8144a6..0x814513 yaw; 0x8145..0x8145f6 pitch] Rate, per axis:
    ///    `|err| <= 1e-4`: rate = 0.
    ///    `err < 0`: rate = min(0, max(-cap, rate - accel*dt)).
    ///    `err > 0`: rate = max(0, min(cap, rate + accel*dt)).
    ///    `cap` is the max rate (`+0x60` yaw, `+0x68` pitch), `accel` is `+0x64` yaw, `+0x6c`
    ///    pitch [game]. There is NO slowing by the error here: the rate runs to the cap and the
    ///    move below stops it at the target.
    /// 5. [game 00813e30 0x8145f6..0x814622 pitch, 0x814608 yaw] Move: `mv = rate*dt` (rad per
    ///    step), but `mv = err` when `|mv| >= |err|` (never past the target) [game].
    /// 6. [game 00813e30 0x814614, 0x81462d] Angle: `angle = wrap(angle + mv)`, the same wrap,
    ///    NOT a clamp. The rotator's min and max (`+0x50..+0x5c`) are not read by 00813e30.
    pub fn step(&mut self, dt: f32, from: Vec3, target: Option<Vec3>) {
        let (want_yaw, want_pitch) = match target {
            Some(p) => {
                let d = p - from;
                (
                    (-d.x).atan2(d.y),
                    (-d.z).atan2((d.x * d.x + d.y * d.y).sqrt()),
                )
            }
            // [game 00813e30 l.36, 102-106; 008153e0] With no target point the game aims 10 m
            // along the rotator bone's +y in its BASE pose, so the turret turns back to its rest
            // facing. Here that point is `rest` (set from the launch) [stand-in until set].
            None => {
                let d = self.rest;
                (
                    (-d.x).atan2(d.y),
                    (-d.z).atan2((d.x * d.x + d.y * d.y).sqrt()),
                )
            }
        };
        if self.max_rate_yaw <= 0.0 || self.max_rate_pitch <= 0.0 {
            // [game 00813e30 0x8143d4] no rate cap set: the angles are used as they are.
            self.yaw = want_yaw;
            self.pitch = want_pitch;
            return;
        }
        let err_yaw = wrap_angle(want_yaw - self.yaw);
        let err_pitch = wrap_angle(want_pitch - self.pitch);
        self.yaw_rate = approach(self.yaw_rate, err_yaw, self.accel_yaw, self.max_rate_yaw, dt);
        self.pitch_rate = approach(
            self.pitch_rate,
            err_pitch,
            self.accel_pitch,
            self.max_rate_pitch,
            dt,
        );
        let move_yaw = clamp_move(self.yaw_rate * dt, err_yaw);
        let move_pitch = clamp_move(self.pitch_rate * dt, err_pitch);
        self.yaw = wrap_angle(self.yaw + move_yaw);
        self.pitch = wrap_angle(self.pitch + move_pitch);
    }
}

impl Rotator {
    /// The way the turret points, from its yaw and pitch. The inverse of the desired angles of
    /// `step`: `yaw = atan2(-dx, dy)`, `pitch = atan2(-dz, hypot(dx, dy))` [game 00813e30 b], so
    /// `d = (-sin yaw cos pitch, cos yaw cos pitch, -sin pitch)`. [game 00813e30 l.250-300] the
    /// rotator sets its bone's world basis to Rz(yaw) after Rx(-pitch) (glam order
    /// `Rz(yaw) * Rx(-pitch)`, trackParent off), whose +y is exactly this. The game's cone test
    /// (`in_fire_cone`) does NOT use this: `FUN_00815290` takes the +y row of node `ft+0x48`
    /// [game 00815290, 0079f0f0 l.80-107]. For a turret with no bound weapon (Blackout's, name
    /// 0) `ft+0x48` stays 0 [game 0079dda0 l.30-38: its only write is the copy of `local_5c`,
    /// which is 0]: so the cone is taken from node 0 [game 00815290 l.2-3, the index test].
    /// `FUN_011749d0` with index 0 copies the object's own matrix (else entry N-1 composed with
    /// it, the rule of 0056bee0) [game 011749d0, read 2026-10-20], and 00815290 takes its +y row
    /// (+0xc..+0x14): the cone is about the turret object's +y, its rest facing. The game's
    /// turret uses `Rotator::rest` for that test (game\src\weapons.rs `tick_turrets`), not this.
    pub fn facing(&self) -> Vec3 {
        Vec3::new(
            -self.yaw.sin() * self.pitch.cos(),
            self.yaw.cos() * self.pitch.cos(),
            -self.pitch.sin(),
        )
    }
}

/// The rate update of `Rotator::step` (steps 4 of its doc) [game 00813e30].
fn approach(rate: f32, err: f32, accel: f32, cap: f32, dt: f32) -> f32 {
    if err.abs() <= 1.0e-4 {
        0.0
    } else if err < 0.0 {
        (rate - accel * dt).max(-cap).min(0.0)
    } else {
        (rate + accel * dt).max(0.0).min(cap)
    }
}

/// The move this step: `mv`, but `err` when `|mv| >= |err|` (never past the target)
/// [game 00813e30 0x8145f6..0x814622, 0x814608].
fn clamp_move(mv: f32, err: f32) -> f32 {
    if mv.abs() > err.abs() {
        err
    } else {
        mv
    }
}

/// An angle wrapped to (-pi, pi].
fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// The fire timer of a FireTurret [game 0079e670]: the countdown goes down each step while the
/// turret is enabled; at zero it fires and is reset to `1 / fireRate` (no carry; 0 when the
/// rate is 0). So `fireRate` is a countdown reset, not a shots-per-second count: above about
/// 31 (one per 0.032 s step) the turret fires once per step.
#[derive(Clone, Debug, PartialEq)]
pub struct FireTimer {
    /// `fireRate` (data 100) [data].
    pub fire_rate: f32,
    /// `ft+0x50`, seconds to the next shot [game 0079e670].
    pub countdown: f32,
}

impl FireTimer {
    /// The countdown starts at 0, so the first enabled step fires [game 0079dbc0: the FireTurret
    /// constructor zeroes `ft+0x50` (`param_1[0x14]`); its loader 0079e250 does not write it].
    pub fn new(fire_rate: f32) -> Self {
        FireTimer {
            fire_rate,
            countdown: 0.0,
        }
    }

    /// One step. `enabled` stands for the game's gates [game 0079e670]: `ft+0x4c` set, `ft+0x7d`
    /// zero, and the owner not disabled. Returns true on the step the fire call happens
    /// (vtable `+0x24`, not identified: what the shot is is [unknown]).
    pub fn step(&mut self, dt: f32, enabled: bool) -> bool {
        if !enabled {
            return false;
        }
        self.countdown -= dt;
        if self.countdown <= 0.0 {
            self.countdown = if self.fire_rate != 0.0 {
                1.0 / self.fire_rate
            } else {
                0.0
            };
            true
        } else {
            false
        }
    }
}

/// The fire turret's band test, target_score 007996c0, for one candidate [game]. `owner` is the
/// turret's position and `facing` the centre of its arc, a unit vector. The query the game builds
/// [game 0079e920 l.34-48, raw disasm 0079ea8a-0079eaf2] has: ESI[5] = ft+0x64 (band top),
/// ESI[6] = ft+0x60 (band bottom), ESI[7] = ft+0x60 (the score's reference), ESI[8] = ft+0x64 (the
/// bound on |dz|), ESI[10] = ft+0x68 (yaw half-limit) and ESI[12] = ft+0x6c (pitch half-limit).
///
/// The test, strict at each bound: `reach = |flat - radius|` where `flat` is the horizontal
/// distance, must lie in `(min_range, max_range)`; `|dz| < max_range`; the yaw of the candidate
/// from the owner, `atan2(-dx, dy)`, less the centre's yaw, wrapped, has magnitude `< max_yaw`;
/// the pitch, `atan2(-dz, hypot(dx, dy))`, less the centre's, has magnitude `< max_pitch`. The
/// score is `|reach - min_range|` [game 007996c0 l.27, `fabs(fVar2 - ESI[7])`].
pub fn band_score(band: &FireBand, owner: Vec3, facing: Vec3, at: Vec3, radius: f32) -> Option<f32> {
    let d = at - owner;
    let flat = (d.x * d.x + d.y * d.y).sqrt();
    let reach = (flat - radius).abs();
    if !(band.min_range < reach && reach < band.max_range) {
        return None;
    }
    if d.z.abs() >= band.max_range {
        return None;
    }
    // The centre: the yaw and pitch of `facing` in the same convention [game 0079e920 l.29-33].
    let yaw_centre = (-facing.x).atan2(facing.y);
    let pitch_centre = (-facing.z).atan2((facing.x * facing.x + facing.y * facing.y).sqrt());
    // The candidate's angles; the game normalises d first, which does not change atan2 [game].
    let yaw = (-d.x).atan2(d.y);
    let pitch = (-d.z).atan2(flat);
    if wrap_angle(yaw - yaw_centre).abs() >= band.max_yaw || wrap_angle(pitch - pitch_centre).abs() >= band.max_pitch {
        return None;
    }
    Some((reach - band.min_range).abs())
}

/// Target choice of a FireTurret [game 0079e920, 00799cd0]: the candidates of the enemy team that
/// are present and visible (`visible` stands for the line-of-sight test when `checkLineOfSight` is
/// set; the caller makes it) and pass `band_score`, in their walk order, at most five [game
/// 00799cd0 `param_5` = 5: the walk returns once five are kept]. Of those, the lowest score wins,
/// the first on a tie [game 0079e920 l.126, `fStack_110`, strict `<`]. The owner itself is not a
/// candidate.
///
/// The game's pick has NO cone test: `facing` is the arc's centre only. The cone is the fire gate
/// of `in_fire_cone`, which the game applies only in its TurretManager mode (`ft+0x94` set) [game
/// 0079f0f0 l.10-16, 107]. The centre is FUN_0079f5f0's direction: with `ft+0x94` clear the
/// object's +y row, normalised [game 0079f5f0 l.20-30]. The game's own name for that row is the
/// "object forward row (+0x1c..+0x24)" [game 00815290], and the port's y is forward, so the two are
/// the same row. The no-target aim is that same direction [game 0079e920 l.94-103], so the caller's
/// rest facing is that row [game 0079e920 l.94-103, 00815290].
/// With `ft+0x94` set the centre is the rotator cone instead: 00815290 takes the +y row of node +0x48
/// when valid [game 0079f5f0 l.4-14, 00815290]. The port's rotator-mode centre is [assumed: the rest
/// facing]. settles: the rotator node +0x48 row as the caller passes it (game/src/weapons.rs), not read here.
/// Read (2026-10-20): in that mode 0079f5f0 walks the TurretRotator objects (007a0d50) for one with +0x4d and
/// +0xcc set (else any with +0xcc) and calls 00815290 on THAT object, so the node index is the rotator's own
/// +0x48 (FireTurret's `ft+0x48` = 0 does not decide it); if that node turns with the rotator's yaw, the centre
/// follows the live aim, not the rest facing. settles: the TurretRotator's +0x48 (its loader / blackout.txt).
/// `ft+0x94` is written by the bind 0079dda0 at 0079e017: set when the scene
/// has a TurretRotator (007a0d00, class dd043c1b) [game, read 2026-10-20; the third pass missed
/// it]. `blackout_turret` has one (docs/special-weapons.md), so Blackout's turret is in that mode:
/// the tick picks every update while enabled (0079e670 l.3-5), the rotator slews to `ft+0x80`
/// (00813e30 l.38-48), and each shot needs `in_fire_cone` with a rotator enabled (`+0xcc`, set by
/// the same Trigger as the FireTurret: 00815150 at vtable 00b618b4) [game 0079f0f0].
pub fn pick_target(
    position: Vec3,
    facing: Vec3,
    candidates: &[Candidate],
    band: &FireBand,
    enemy_team: u32,
) -> Option<u32> {
    let mut best: Option<(f32, u32)> = None;
    let mut kept = 0;
    for &(id, team, at, visible, radius) in candidates {
        if kept >= 5 {
            break;
        }
        if team != enemy_team || !visible {
            continue;
        }
        let Some(score) = band_score(band, position, facing, at, radius) else {
            continue;
        };
        if score >= 1000.0 {
            continue;
        }
        kept += 1;
        if best.is_none_or(|(bs, _)| score < bs) {
            best = Some((score, id));
        }
    }
    best.map(|(_, id)| id)
}

/// The fire gate of the game's TurretManager mode [game 0079f0f0 l.106-108]: the aim direction
/// (`aim - from`) and the weapon's +y direction, both normalised, must have a dot above
/// `cos(cone)`. `aim` and `facing` are the vectors as given; a zero one is left as it is, so
/// its dot is 0 [game: `vec4_normalize_to` changes nothing below 1e-8 length].
pub fn in_fire_cone(from: Vec3, aim: Vec3, facing: Vec3, cone_deg: f32) -> bool {
    let d = aim - from;
    let d = if d.length() > 1e-8 { d.normalize() } else { d };
    let f = if facing.length() > 1e-8 { facing.normalize() } else { facing };
    d.dot(f) > cone_deg.to_radians().cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> LauncherTuning {
        LauncherTuning {
            launch_speed: 50.0,
            z_gravity: -64.0,
            life_time: -1.0,
            max_turrets_at_once: 1,
            missing: Vec::new(),
        }
    }

    #[test]
    fn a_launched_turret_follows_v0_under_gravity() {
        let t0 = tuning().launch(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.6, 0.8), 0.0, 0.5);
        assert!((t0.velocity - Vec3::new(0.0, 30.0, 40.0)).length() < 1e-4);
        let mut t = t0;
        for _ in 0..16 {
            t.step(1.0 / 32.0, None);
        }
        // The game's order [game 006e96c0, 006de230, 006ddad0]: the damping of the velocity, then
        // the gravity into it, then the position moves by the new velocity. Written out per step
        // in f64, with no damping the z would be 21.5 (vz 8); the damping of 0.9995 a step takes
        // a little off each of them.
        let (mut vy, mut vz, mut y, mut z) = (30.0f64, 40.0f64, 0.0f64, 10.0f64);
        for _ in 0..16 {
            vy *= 0.9995;
            vz = vz * 0.9995 - 64.0 / 32.0;
            y += vy / 32.0;
            z += vz / 32.0;
        }
        assert!((t.position.y as f64 - y).abs() < 1e-4, "y {} vs {y}", t.position.y);
        assert!((t.position.z as f64 - z).abs() < 1e-4, "z {} vs {z}", t.position.z);
        assert!((t.velocity.z as f64 - vz).abs() < 1e-4);
        assert!((z - 21.5).abs() < 0.1, "damping only takes a little: {z}");
        assert_eq!(t.state, State::Flying);
    }

    #[test]
    fn a_turret_lands_on_flat_ground_then_deploys_and_is_active() {
        let mut t = tuning().launch(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.6, 0.8), 0.0, 0.5);
        let mut steps = 0;
        while t.state == State::Flying && steps < 2000 {
            let contact = if t.position.z <= 0.0 { Some(Vec3::Z) } else { None };
            t.step(1.0 / 32.0, contact);
            steps += 1;
        }
        assert_eq!(t.state, State::Deploying);
        assert_eq!(t.velocity, Vec3::ZERO);
        let at = t.position;
        for _ in 0..16 {
            t.step(1.0 / 32.0, None);
        }
        assert_eq!(t.state, State::Active);
        assert_eq!(t.position, at);
    }

    #[test]
    fn a_wall_contact_does_not_land_it_and_a_steep_one_does() {
        let mut wall = tuning().launch(Vec3::new(0.0, 0.0, 5.0), Vec3::X, 0.0, 0.5);
        for _ in 0..10 {
            wall.step(1.0 / 32.0, Some(Vec3::new(0.0, 0.9, 0.4)));
        }
        assert_eq!(wall.state, State::Flying);
        // Normalised z is 0.6 > 0.5: lands.
        let mut steep = tuning().launch(Vec3::new(0.0, 0.0, 5.0), Vec3::X, 0.0, 0.5);
        steep.step(1.0 / 32.0, Some(Vec3::new(0.0, 0.8, 0.6)));
        assert_eq!(steep.state, State::Deploying);
        // The landing's up row is the normal, normalised [game 0078b8e0 l.70-80]: a slope of
        // normal (0, 0.8, 0.6) gives that up, not world z.
        assert!((steep.up - Vec3::new(0.0, 0.8, 0.6)).length() < 1e-6, "up {:?}", steep.up);
        // In flight the up stays world z.
        assert_eq!(wall.up, Vec3::Z);
    }

    #[test]
    fn the_rest_facing_from_the_frame_takes_the_cross_case_when_steep() {
        // A flat-ish throw: the flat +y, turned back by Rz(pi) [game 0078c2e0].
        let flat = rest_facing_for_frame(Vec3::new(0.0, 0.6, 0.8), Vec3::X);
        assert!((flat - Vec3::NEG_Y).length() < 1e-6, "flat {flat:?}");
        // Vertical throw, x row +x: normal x x = +y, so the base faces -y.
        let up_x = rest_facing_for_frame(Vec3::Z, Vec3::X);
        assert!((up_x - Vec3::NEG_Y).length() < 1e-6, "vertical {up_x:?}");
        // Vertical throw, x row +y: normal x y = -x, so the base faces +x.
        let up_y = rest_facing_for_frame(Vec3::Z, Vec3::Y);
        assert!((up_y - Vec3::X).length() < 1e-6, "vertical y {up_y:?}");
        // The threshold is |dot| > 0.96 [0056e7b0 l.9]: 0.97 up uses the cross case.
        let steep = rest_facing_for_frame(Vec3::new(0.0, 0.2431, 0.97), Vec3::Y);
        assert!((steep - Vec3::X).length() < 1e-3, "steep {steep:?}");
    }

    #[test]
    fn the_cap_removes_only_the_oldest_once_per_call() {
        let tn = tuning();
        let mut list = vec![
            tn.launch(Vec3::ZERO, Vec3::Z, 1.0, 0.5),
            tn.launch(Vec3::ZERO, Vec3::Z, 2.0, 0.5),
            tn.launch(Vec3::ZERO, Vec3::Z, 3.0, 0.5),
        ];
        enforce_cap(&mut list, 2);
        let times: Vec<f32> = list.iter().map(|t| t.spawned_at).collect();
        assert_eq!(times, vec![2.0, 3.0]);
        enforce_cap(&mut list, 2);
        assert_eq!(list.len(), 2);
        enforce_cap(&mut list, 1);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].spawned_at, 3.0);
    }

    #[test]
    fn the_rotator_turns_70_degrees_in_a_second_and_no_faster() {
        // Accelerations large enough that the cap is reached at once (see Rotator::step).
        let mut r = Rotator::from_data_degrees([70.0, 50.0], [1.0e6, 1.0e6]);
        // Target at yaw +170 degrees (d = (-sin 170, -cos 170, 0)). The error stays above the
        // 70 deg/s rate for the whole second, so the cap is what limits the turn. [game 00813e30]
        // The rate does not slow for the error; the move is cut to the error only at the target.
        let target = Some(Vec3::new(-0.173_648_18, -0.984_807_75, 0.0));
        r.step(1.0 / 32.0, Vec3::ZERO, target);
        let first = r.yaw.to_degrees();
        assert!(first > 0.0 && first <= 70.0 / 32.0 + 1e-3, "faster than 70 deg/s: {first}");
        for _ in 0..31 {
            r.step(1.0 / 32.0, Vec3::ZERO, target);
        }
        assert!((r.yaw.to_degrees() - 70.0).abs() < 1e-2, "yaw {}", r.yaw.to_degrees());
        assert!(r.pitch.abs() < 1e-6);
    }

    #[test]
    fn the_rotator_does_not_slow_for_the_error_and_stops_on_the_target() {
        // [game 00813e30] the rate runs to the cap; the move is cut to the error (never past it),
        // and once the error is within 1e-4 the rate is 0. Target 10 degrees away at the cap of
        // 70 deg/s: steps of 70/32 degrees, so the 5th step lands on 10 degrees.
        let mut r = Rotator::from_data_degrees([70.0, 50.0], [1.0e6, 1.0e6]);
        let target = Some(Vec3::new(-10f32.to_radians().sin(), 10f32.to_radians().cos(), 0.0));
        for _ in 0..4 {
            r.step(1.0 / 32.0, Vec3::ZERO, target);
        }
        assert!((r.yaw_rate.to_degrees() - 70.0).abs() < 1e-3, "rate {}", r.yaw_rate.to_degrees());
        r.step(1.0 / 32.0, Vec3::ZERO, target);
        assert!((r.yaw.to_degrees() - 10.0).abs() < 1e-3, "yaw {}", r.yaw.to_degrees());
        r.step(1.0 / 32.0, Vec3::ZERO, target);
        assert_eq!(r.yaw_rate, 0.0, "error within 1e-4: the rate is 0");
    }

    #[test]
    fn the_rotator_with_no_rate_cap_snaps_to_the_target() {
        // [game 00813e30 0x8143c3..0x8143d4] a cap of 0 uses the desired angles directly.
        let mut r = Rotator::from_data_degrees([0.0, 50.0], [70.0, 50.0]);
        let target = Some(Vec3::new(-10f32.to_radians().sin(), 10f32.to_radians().cos(), 0.0));
        r.step(1.0 / 32.0, Vec3::ZERO, target);
        assert!((r.yaw.to_degrees() - 10.0).abs() < 1e-3, "yaw {}", r.yaw.to_degrees());
    }

    #[test]
    fn the_stored_acceleration_limits_the_first_second() {
        // [game 00813e30 step 4] the accel as stored (70 deg/s^2, 008139b0 times DEG): the rate
        // ramps up by accel*dt a step to the cap, so the first second turns about 36 degrees,
        // not 70.
        let mut r = Rotator::from_data_degrees([70.0, 50.0], [70.0, 50.0]);
        for _ in 0..32 {
            r.step(1.0 / 32.0, Vec3::ZERO, Some(Vec3::new(1.0, 0.0, 0.0)));
        }
        let turned = r.yaw.to_degrees().abs();
        assert!(turned > 35.0 && turned < 37.0, "turned {turned}");
    }

    #[test]
    fn the_fire_timer_fires_once_per_step_at_rate_100() {
        let mut f = FireTimer::new(100.0);
        for _ in 0..5 {
            assert!(f.step(0.032, true));
        }
        assert!(!f.step(0.032, false), "a disabled turret does not fire");
    }

    #[test]
    fn the_fire_timer_at_rate_1_fires_every_fourth_step_of_a_quarter_second() {
        let mut f = FireTimer::new(1.0);
        let shots = (0..8).filter(|_| f.step(0.25, true)).count();
        assert_eq!(shots, 2);
    }

    #[test]
    fn the_target_is_the_nearest_visible_enemy_in_range_and_cone() {
        let me = Vec3::ZERO;
        let facing = Vec3::Y;
        // Blackout's band: 0 to 300 m, the full circle [data blackout.txt 720724-720728].
        let open = FireBand { min_range: 0.0, max_range: 300.0, max_yaw: TAU, max_pitch: TAU };
        let c = [
            (1, 7, Vec3::new(0.0, 50.0, 0.0), true, 0.5),   // enemy, 50 m: score 49.5
            (2, 7, Vec3::new(0.0, 20.0, 0.0), true, 0.5),   // enemy, 20 m: score 19.5, wins
            (3, 9, Vec3::new(0.0, 5.0, 0.0), true, 0.5),    // wrong team
            (4, 7, Vec3::new(0.0, 400.0, 0.0), true, 0.5),  // out of range
            (5, 7, Vec3::new(0.0, 10.0, 0.0), false, 0.5),  // not visible
            (6, 7, Vec3::new(30.0, 10.0, 0.0), true, 0.5),  // 31.1 m, off the facing: still a target
        ];
        assert_eq!(pick_target(me, facing, &c, &open, 7), Some(2));
        // [game 0079e920] the pick has no cone: the one off the facing is taken.
        assert_eq!(pick_target(me, facing, &c[5..], &open, 7), Some(6));
        assert_eq!(pick_target(me, facing, &c[2..3], &open, 7), None);
        // The score is the distance from the band's bottom: with a 25 m bottom, 20 m is outside
        // the band and 50 m (score 24.5) is the one taken.
        let inner = FireBand { min_range: 25.0, ..open };
        assert_eq!(pick_target(me, facing, &c[..2], &inner, 7), Some(1));
        // The yaw limit: 60 degrees either side of the facing; the candidate at -71.6 degrees is out.
        let narrow = FireBand { max_yaw: 60f32.to_radians(), ..open };
        assert_eq!(pick_target(me, facing, &c[5..], &narrow, 7), None);
        assert_eq!(pick_target(me, facing, &c[..1], &narrow, 7), Some(1));
        // The |dz| bound is the band top (300): a candidate 400 m up is out, 200 m up is in.
        let up = [(8, 7, Vec3::new(0.0, 20.0, 400.0), true, 0.5), (9, 7, Vec3::new(0.0, 20.0, 200.0), true, 0.5)];
        assert_eq!(pick_target(me, facing, &up, &open, 7), Some(9));
        // The candidate's radius is taken off its distance: a 10 m radius at 20 m centre is 10 m.
        assert!((band_score(&open, me, facing, Vec3::new(0.0, 20.0, 0.0), 10.0).unwrap() - 10.0).abs() < 1e-4);
        // The fire gate (TurretManager mode) [game 0079f0f0]: inside 15 degrees, and not outside.
        assert!(in_fire_cone(me, Vec3::new(0.0, 20.0, 0.0), facing, 15.0));
        assert!(!in_fire_cone(me, Vec3::new(30.0, 10.0, 0.0), facing, 15.0));
    }

    #[test]
    fn a_burst_of_three_at_point_zero_eight_second_gaps() {
        let mut b = Burst::default();
        b.start(3, 0.08);
        let dt = crate::sim::GAME_UPDATE;
        let shots: Vec<u32> = (0..10).map(|_| b.step(dt)).collect();
        // Round 1 at once; round 2 on the third update (clock 0.096, 0.016 carried); round 3 when
        // the clock reaches 0.08 again, two or three updates on (float sums). [game 007ac370]
        println!("burst rounds by update: {shots:?}");
        assert_eq!(shots.iter().sum::<u32>(), 3);
        assert_eq!(shots[0], 1);
        assert_eq!(shots[3], 1, "second round after 0.096 s");
        assert!(shots[5] == 1 || shots[6] == 1, "third round 0.064 or 0.096 s later");
        assert_eq!(b.left, 0);
        // At 60 frames a second the rounds still come on whole updates.
        let mut b = Burst::default();
        b.start(3, 0.08);
        let frames: Vec<u32> = (0..30).map(|_| b.step(1.0 / 60.0)).collect();
        assert_eq!(frames.iter().sum::<u32>(), 3);
        assert_eq!(frames[0], 1);
    }

    #[test]
    fn the_cap_index_is_the_oldest_and_none_within_the_max() {
        let tn = tuning();
        let list = vec![
            tn.launch(Vec3::ZERO, Vec3::Z, 5.0, 0.5),
            tn.launch(Vec3::ZERO, Vec3::Z, 2.0, 0.5),
        ];
        assert_eq!(cap_index(&list, 2), None);
        assert_eq!(cap_index(&list, 1), Some(1));
    }

    #[test]
    fn the_rotator_faces_the_way_its_angles_point() {
        let mut r = Rotator::from_data_degrees([1.0e6, 1.0e6], [1.0e6, 1.0e6]);
        assert!((r.facing() - Vec3::Y).length() < 1e-6);
        r.step(1.0, Vec3::ZERO, Some(Vec3::new(-1.0, 0.0, 0.0)));
        // A target at -x is at yaw +90 degrees (atan2(1, 0)): facing points at it.
        assert!((r.facing() - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-3, "facing {:?}", r.facing());
    }

    #[test]
    fn the_turret_faces_back_along_the_throw_and_turns_to_it_with_no_target() {
        // [game 0078c2e0 l.15-31] the base faces back along the throw: a throw along +y gives a
        // rest facing of -y.
        let t = tuning().launch(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), 0.0, 0.5);
        assert!((t.rest_facing - Vec3::NEG_Y).length() < 1e-6, "rest {:?}", t.rest_facing);
        // Turned away, then with no target it turns back to the rest facing [game 00813e30].
        let mut r = Rotator::from_data_degrees([1.0e6, 1.0e6], [1.0e6, 1.0e6]);
        r.rest = t.rest_facing;
        r.yaw = 0.5;
        for _ in 0..4 {
            r.step(1.0 / 32.0, Vec3::ZERO, None);
        }
        assert!((r.yaw.abs() - PI).abs() < 1e-3, "yaw {}", r.yaw);
        assert!(r.pitch.abs() < 1e-6);
        // A vertical throw has no flattened direction: the facing is +y [stand-in], as `Turret::rest_facing` (l.172).
        let up = tuning().launch(Vec3::ZERO, Vec3::Z, 0.0, 0.5);
        assert_eq!(up.rest_facing, Vec3::Y);
    }
}
