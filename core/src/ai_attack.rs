//! The AI attack and simple behaviours (phase 3): AttackRanged, AttackMelee, AttackTransform,
//! WeaponSwitch, Idle and Dodge. Source: notes/ai-behaviours-attack.md (its sections 0 to 9), and
//! notes/ai-brain.md "The tick, settled" for how the brain runs them.
//!
//! A behaviour acts only by pushing `Request`s into Ctx, and by the Ctx clocks and flags it is
//! given. It never touches a character. Tags as in ai_brain.rs. Not wired into the game yet.

use crate::ai_brain::{Behaviour, Ctx, Request, SelfState, Target};
use crate::ai_perceive::Aggregate;

/// Button id 1: AttackFast, melee kind 1 [game; notes/state-machine.md enum].
pub const BTN_ATTACK_FAST: u32 = 1;
/// Button id 2: AttackSpecial, melee kind 2 [game].
pub const BTN_ATTACK_SPECIAL: u32 = 2;
/// Button id 3: AttackVehicle [game].
pub const BTN_ATTACK_VEHICLE: u32 = 3;
/// Button id 0x15, VehicleMode in the enum. Pressed by Idle's aim helper 0082ab50 [game]. The weapon
/// mode request 0082ac80 only TESTS it (no weapon mode press while it is down) and presses 0x18
/// [game, raw 0082ad18; settled 2026-10-20, ai-behaviours-attack.md section 7 item 1].
pub const BTN_VEHICLE_MODE: u32 = 0x15;
/// Button id 0x17: WeaponFire [game].
pub const BTN_WEAPON_FIRE: u32 = 0x17;
/// Button id 0x21: AttackTransformHover [game].
pub const BTN_HOVER: u32 = 0x21;
/// Button id 0x22: Jump [game].
pub const BTN_JUMP: u32 = 0x22;
/// Button id 0x23: Punch [game].
pub const BTN_PUNCH: u32 = 0x23;
/// Button id 0x24: Slam [game]. Has no data object (ai-behaviours-attack.md section 3).
pub const BTN_SLAM: u32 = 0x24;
/// Button id 0: Action (climb, drop, pickup, throw) [game, notes/state-machine.md enum].
pub const BTN_ACTION: u32 = 0;
/// Button id 0x18: WeaponMode [game 0082ab50 clears bit 0x18, ai-targeting-idle.md B.2].
pub const BTN_WEAPON_MODE: u32 = 0x18;

/// State hash StateAttack [game 0x3e5bcf4a, ai-behaviours-attack.md section 1 item 11].
pub const STATE_ATTACK: u32 = 0x3e5bcf4a;
/// State hash StateDash [game 0x7374abee, section 6 item 3].
pub const STATE_DASH: u32 = 0x7374abee;

/// The aim cone range, metres [game 0x461c4000 = 10000.0, ai-behaviours-attack.md section 0].
const AIM_RANGE: f32 = 10000.0;
/// The foe range of the shared attack gate, metres [game 0x459c4000 = 5000.0].
const FOE_RANGE: f32 = 5000.0;

fn is_transform_kind(kind: u32) -> bool {
    (BTN_HOVER..=BTN_SLAM).contains(&kind)
}

/// The kung-fu slot gate 008254c0 [unread]. [stand-in] passes.
fn kung_fu_gate(_ctx: &Ctx) -> bool {
    true
}

/// ai_shared_attack_gate 00824a00 [game, decompile]: passes when the attack delay has run (or is
/// ignored) and weapons are not disabled, and there is a foe target within 0..5000. The game also
/// sets the target here (00824830); [stand-in] the caller sets `ctx.target`.
fn shared_attack_gate(ctx: &Ctx, ignore_delay: bool) -> bool {
    if !ignore_delay && ctx.attr.attack_delay > ctx.attack_clock {
        return false;
    }
    if ctx.weapons_disabled {
        return false;
    }
    match ctx.target {
        Some(t) => (0.0..=FOE_RANGE).contains(&t.distance),
        None => false,
    }
}

/// ai_attack_priority 0081b850 [game]: the perception score, then the kung-fu gate unless the
/// behaviour ignores it. The mark-foe call (008252c0) is [unread] and not made.
fn attack_priority(perception: &Aggregate, ctx: &Ctx, ignore_kung_fu: bool) -> f32 {
    let s = perception.score(ctx);
    if s <= 0.0 {
        return 0.0;
    }
    if !ignore_kung_fu && !kung_fu_gate(ctx) {
        return 0.0;
    }
    s
}

/// The aim cone test 00723c30 with useWeaponAim 0 [game]: the target is within `max_dist`
/// horizontally of the camera target point, and inside the cone of half-angle `half_angle` around
/// the heading. The facing is (-sin, cos) of the heading [game per the notes]. The cone uses
/// cos(half_angle) [assumed] that the attribute is the half-angle in radians.
fn in_cone(me: &SelfState, target: &Target, max_dist: f32, half_angle: f32) -> bool {
    let o = me.aim_origin;
    let dx = target.pos[0] - o[0];
    let dy = target.pos[1] - o[1];
    let d = (dx * dx + dy * dy).sqrt();
    if d > max_dist {
        return false;
    }
    if d == 0.0 {
        return true;
    }
    let (s, c) = me.facing.sin_cos();
    let cos_angle = (dx * -s + dy * c) / d;
    cos_angle >= half_angle.cos()
}

/// ai_attack_request for kind 0x17 (008297e0) [game]: refused while input is blocked, in a
/// vehicle, or with weapons disabled. Else a weapon mode request with no index, then the fire
/// button, unless it is already held.
/// [stand-in] the weapon sub-object test (FUN_00721a60, +0x3a0 byte 0x18) and the weapon's target
/// test (007b36c0, 0070a3c0, 00562970) are not ported: the button is pressed unless held.
fn fire_request(ctx: &mut Ctx) {
    if ctx.input_blocked || ctx.me.in_vehicle || ctx.weapons_disabled {
        return;
    }
    ctx.requests.push(Request::WeaponMode(-1));
    if ctx.is_held(BTN_WEAPON_FIRE) {
        return;
    }
    ctx.requests.push(Request::Button(BTN_WEAPON_FIRE));
}

/// ai_attack_request for melee kinds 1 and 2 (008297e0) [game]: refused while input is blocked;
/// not pressed again while held; else the button is pressed.
fn melee_request(ctx: &mut Ctx, kind: u32) {
    if ctx.input_blocked {
        return;
    }
    if kind != BTN_ATTACK_VEHICLE && ctx.is_held(kind) {
        return;
    }
    ctx.requests.push(Request::Button(kind));
}

/// AttackRanged (24 objects; vtable 00b61b20) [game]. Data: ignoreAttackDelay true (24 of 24),
/// ignoreKungFuFighting false, weaponIndex -1. The kind is always WeaponFire 0x17 (ctor 0081be70).
pub struct AttackRanged {
    /// +0x28 ignoreAttackDelay.
    pub ignore_attack_delay: bool,
    /// +0x29 ignoreKungFuFighting.
    pub ignore_kung_fu: bool,
    /// +0x2c weaponIndex, -1 for none.
    pub weapon_index: i32,
    /// The perception aggregate (`G+0x10` links) [game 0082e7e0].
    pub perception: Aggregate,
    /// The persistent channel. [unknown] for this type (the slot +0x34 is not read).
    pub channel: usize,
}

impl Behaviour for AttackRanged {
    fn channel(&self) -> usize {
        self.channel
    }

    fn score(&self, ctx: &Ctx) -> f32 {
        if !shared_attack_gate(ctx, self.ignore_attack_delay) {
            return 0.0;
        }
        attack_priority(&self.perception, ctx, self.ignore_kung_fu)
    }

    fn enter(&mut self, ctx: &mut Ctx) {
        self.perception.enter(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) {
        self.perception.leave(ctx);
    }

    fn start(&mut self, _ctx: &mut Ctx) {}

    /// Update 0081bf10 [game, decompile]. With no target nothing is pressed. With the aim test
    /// (attr+0x62c) and the target outside the cone: on foot a weapon mode request, in a vehicle
    /// the vehicle attack button (when input is not blocked). Else the fire request, and the
    /// attack clock is reset.
    fn update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let Some(target) = ctx.target else {
            return;
        };
        if ctx.attr.aim_test && !in_cone(&ctx.me, &target, AIM_RANGE, ctx.attr.aim_cone) {
            if !ctx.me.in_vehicle {
                ctx.requests.push(Request::WeaponMode(self.weapon_index));
            } else if !ctx.input_blocked {
                ctx.requests.push(Request::Button(BTN_ATTACK_VEHICLE));
            }
            // [stand-in] ai_shared_behaviour_helper 00823350 (track and move toward the target) is
            // not ported: no movement request is made.
            return;
        }
        fire_request(ctx);
        ctx.attack_clock = 0.0;
    }

    /// End hook 0081b980 [game]: the attack clock is reset. [stand-in] the target clear
    /// (008253c0, unless in StateAttack) is not ported.
    fn stop(&mut self, ctx: &mut Ctx) {
        ctx.attack_clock = 0.0;
    }

    /// Done 0076c530 = 1 [game]: the goal finishes after one update.
    fn finished(&self, _ctx: &Ctx) -> bool {
        true
    }

    fn interruptible(&self, _ctx: &Ctx) -> bool {
        true
    }
}

/// AttackMelee (5 objects; vtable 00b61acc) [game]. `kind` is 1 (AttackFast) or 2 (AttackSpecial,
/// when the data's attackSpecial is set).
pub struct AttackMelee {
    /// 1 for AttackFast, 2 for AttackSpecial.
    pub kind: u32,
    /// +0x28 ignoreAttackDelay (true for ByMsg, Special and SpecialVehicle).
    pub ignore_attack_delay: bool,
    /// +0x29 ignoreKungFuFighting (false for all five).
    pub ignore_kung_fu: bool,
    pub perception: Aggregate,
    /// [unknown] (see AttackRanged).
    pub channel: usize,
}

impl Behaviour for AttackMelee {
    fn channel(&self) -> usize {
        self.channel
    }

    /// Priority 0081bc80 [game, disasm checked]. The special is blocked while the special object's
    /// float is above 0. Otherwise a fast attack waits for the attack delay, unless a combo window
    /// is open. Then the attack priority.
    fn score(&self, ctx: &Ctx) -> f32 {
        if self.kind == BTN_ATTACK_SPECIAL {
            if ctx.me.special_block > 0.0 {
                return 0.0;
            }
        } else if !self.ignore_attack_delay
            && ctx.attack_clock < ctx.attr.attack_delay
            && !ctx.combo_window
        {
            return 0.0;
        }
        attack_priority(&self.perception, ctx, self.ignore_kung_fu)
    }

    fn enter(&mut self, ctx: &mut Ctx) {
        self.perception.enter(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) {
        self.perception.leave(ctx);
    }

    fn start(&mut self, _ctx: &mut Ctx) {}

    /// Update 0081bbe0 [game]. The request runs when the character is not in StateAttack, or a
    /// combo window is open. A special in a vehicle also presses button 2 first ([stand-in]
    /// 00725890 is not ported). Then the request and the attack clock reset (ctrl+0xcc = 0), both
    /// inside the test [game, raw 0081bc1a `je 0x81bc6d` skips both]: in StateAttack outside a combo
    /// window the clock keeps running.
    fn update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if ctx.me.state != STATE_ATTACK || ctx.combo_window {
            if self.kind == BTN_ATTACK_SPECIAL && ctx.me.in_vehicle {
                ctx.requests.push(Request::Button(BTN_ATTACK_SPECIAL));
            }
            melee_request(ctx, self.kind);
            ctx.attack_clock = 0.0;
        }
    }

    fn stop(&mut self, ctx: &mut Ctx) {
        ctx.attack_clock = 0.0;
    }

    fn finished(&self, _ctx: &Ctx) -> bool {
        true
    }

    fn interruptible(&self, _ctx: &Ctx) -> bool {
        true
    }
}

/// AttackTransform (9 objects; vtable 00b61b78) [game]. `kind` is one of 0x21 (hover family, 7
/// objects), 0x22 (Jump), 0x23 (Punch).
pub struct AttackTransform {
    /// The button kind (0x21 to 0x24) [data attackType].
    pub kind: u32,
    /// +0x2c-equivalent ignoreKungFuFighting (false for all nine).
    pub ignore_kung_fu: bool,
    pub perception: Aggregate,
    /// [unknown] (see AttackRanged).
    pub channel: usize,
}

impl Behaviour for AttackTransform {
    fn channel(&self) -> usize {
        self.channel
    }

    /// Priority 0081c1a0 [game]: the kind must be 0x21 to 0x24 and the character in a vehicle.
    /// Punch and slam take the attack priority; the others the perception score.
    fn score(&self, ctx: &Ctx) -> f32 {
        if !is_transform_kind(self.kind) || !ctx.me.in_vehicle {
            return 0.0;
        }
        if self.kind == BTN_PUNCH || self.kind == BTN_SLAM {
            return attack_priority(&self.perception, ctx, self.ignore_kung_fu);
        }
        self.perception.score(ctx)
    }

    fn enter(&mut self, ctx: &mut Ctx) {
        self.perception.enter(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) {
        self.perception.leave(ctx);
    }

    fn start(&mut self, _ctx: &mut Ctx) {}

    /// Update 0081c170 [game]: in a vehicle, the button gate 008299d0. Input blocked: nothing. The
    /// kind must be a transform kind. The button is pressed unless the device reports it held.
    /// [stand-in] the follow-up call 008223a0 is not ported.
    fn update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if !ctx.me.in_vehicle || ctx.input_blocked || !is_transform_kind(self.kind) {
            return;
        }
        if !ctx.is_held(self.kind) {
            ctx.requests.push(Request::Button(self.kind));
        }
    }

    /// The end hook (vtable +0x24) is not read (21 bytes at 0081c210): [stand-in] no-op.
    fn stop(&mut self, _ctx: &mut Ctx) {}

    fn finished(&self, _ctx: &Ctx) -> bool {
        true
    }

    fn interruptible(&self, _ctx: &Ctx) -> bool {
        true
    }
}

/// WeaponSwitch (16 objects; persistent behaviours) [game]. `weapon_index` is -1 for none.
pub struct WeaponSwitch {
    /// +0x28 weaponIndex [assumed] read from the data's bool as 1 or 0 (ai-behaviours-attack A4).
    pub weapon_index: i32,
    /// +0x2c disableWeapons: the WeaponsOff behaviours.
    pub disable_weapons: bool,
    pub perception: Aggregate,
    /// The persistent channel [unknown] for this type; the caller supplies it.
    pub channel: usize,
}

impl Behaviour for WeaponSwitch {
    fn channel(&self) -> usize {
        self.channel
    }

    /// Priority 00820740 [game]. [stand-in] the table at ctrl+0x6b0 (0 when the entry is below 0
    /// or above 1) is not read: that check is not applied. The perception score is returned.
    fn score(&self, ctx: &Ctx) -> f32 {
        self.perception.score(ctx)
    }

    fn enter(&mut self, ctx: &mut Ctx) {
        self.perception.enter(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) {
        self.perception.leave(ctx);
    }

    /// Start 00820780 [game]: the controller's weapons-disabled flag takes disableWeapons.
    fn start(&mut self, ctx: &mut Ctx) {
        ctx.weapons_disabled = self.disable_weapons;
    }

    /// Update 008206e0 [game]: a switch is asked when weapons are not disabled, a weapon index is
    /// set, the character is in form 2 or the weapon switch attribute is set, and the weapon in
    /// hand is another one. The button id is [unknown] (section 7, item 2): the request names
    /// the index only.
    fn update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if !self.disable_weapons
            && self.weapon_index != -1
            && (ctx.me.form == 2 || ctx.attr.weapon_switch)
            && ctx.me.weapon_index != self.weapon_index
        {
            ctx.requests.push(Request::WeaponSwitch(self.weapon_index));
        }
        ctx.weapons_disabled = self.disable_weapons;
    }

    /// End 008207a0 [game]: the weapons-disabled flag is cleared.
    fn stop(&mut self, ctx: &mut Ctx) {
        ctx.weapons_disabled = false;
    }

    /// Done = 1 [game]: the switch finishes after one update.
    fn finished(&self, _ctx: &Ctx) -> bool {
        true
    }

    fn interruptible(&self, _ctx: &Ctx) -> bool {
        true
    }
}

/// The Idle track, 00823350 then 00823640 [game, ai-targeting-idle.md B.3 and B.4]: when the
/// character's auto facing (`attr+0x62c`) is set and a target exists, the body turns toward it. The
/// turn itself is made by the integrator from `Ctx::face` (combat.rs, rate [assumed]).
/// [stand-in] the state-class gate (00824ba0), the 008242b0 range test (here the foe range 0..5000
/// of the search), and the non-weapon target copy (ctrl+0x48, writer NOT FOUND) are not ported: the
/// held target `Ctx::target` stands for all of them.
fn track_target(ctx: &mut Ctx) {
    if !ctx.attr.auto_facing {
        return;
    }
    let Some(target) = ctx.target else {
        return;
    };
    if !(0.0..=FOE_RANGE).contains(&target.distance) {
        return;
    }
    ctx.face = Some(target.pos);
}

/// Idle (17 objects; vtable 00b61ca0) [game]. Never finishes by itself.
pub struct Idle {
    /// +0x28 trackTarget.
    pub track_target: bool,
    /// +0x29 vehicleMode.
    pub vehicle_mode: bool,
    /// +0x2a weaponMode.
    pub weapon_mode: bool,
    pub perception: Aggregate,
    /// [unknown] (see AttackRanged).
    pub channel: usize,
}

impl Behaviour for Idle {
    fn channel(&self) -> usize {
        self.channel
    }

    /// Priority 0082e7e0 [game]: the perception score only.
    fn score(&self, ctx: &Ctx) -> f32 {
        self.perception.score(ctx)
    }

    fn enter(&mut self, ctx: &mut Ctx) {
        self.perception.enter(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) {
        self.perception.leave(ctx);
    }

    /// Update 0081e570 [game, ai-targeting-idle.md B.1]. With trackTarget, the track first (the
    /// body turns toward the target). Then weapon mode returns at once with a weapon mode request.
    /// Else vehicle mode, or the Idle aim attribute, runs the aim helper 0082ab50: it presses button
    /// 0x15 and releases 0x18 [game B.2]. Its call 0082ac40 is not ported (param3 is not passed).
    fn update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if self.track_target {
            track_target(ctx);
        }
        if self.weapon_mode {
            ctx.requests.push(Request::WeaponMode(-1));
            return;
        }
        if self.vehicle_mode || ctx.attr.idle_aim {
            ctx.requests.push(Request::Button(BTN_VEHICLE_MODE));
            ctx.requests.push(Request::Release(BTN_WEAPON_MODE));
        }
    }

    fn start(&mut self, _ctx: &mut Ctx) {}

    fn stop(&mut self, _ctx: &mut Ctx) {}

    /// Done 00929ff0 = 0 [game]: Idle ends only when the brain selects another goal.
    fn finished(&self, _ctx: &Ctx) -> bool {
        false
    }

    fn interruptible(&self, _ctx: &Ctx) -> bool {
        true
    }
}

/// Dodge (2 objects: behaviorDodge and behaviorDodgeFwd; vtable 00b61bc4) [game].
pub struct Dodge {
    /// Window weights in the order left, right, forward, backward [data behaviour_params.tsv].
    pub weights: [f32; 4],
    /// The direction code picked at start (0 left, 1 right, 2 forward, 3 backward) [assumed].
    pub dir: u8,
    pub perception: Aggregate,
    /// [unknown] (see AttackRanged).
    pub channel: usize,
}

impl Dodge {
    /// The total of the four windows (loader, `+0x38`) [game].
    pub fn total(&self) -> f32 {
        self.weights.iter().sum()
    }

    /// Direction pick 0081c5e0 [game, partly]: r = a draw over the total; the windows are left,
    /// right, forward, backward in that order. A total of 0 picks nothing [assumed].
    /// [stand-in] the landing test (navmesh 006fb450, FUN_00827ce0, unread) is not ported: the
    /// pick is accepted.
    pub fn pick(&self, ctx: &Ctx) -> Option<u8> {
        let total = self.total();
        if !(total > 0.0) {
            return None;
        }
        let r = ctx.random * total;
        let [left, right, forward, _backward] = self.weights;
        let code = if r < left {
            0
        } else if r < left + right {
            1
        } else if r < left + right + forward {
            2
        } else {
            3
        };
        Some(code)
    }
}

impl Behaviour for Dodge {
    fn channel(&self) -> usize {
        self.channel
    }

    /// Priority 0081c4f0 [game]: needs the dodge threshold at or below the dodge clock, a current
    /// state, and StateDash possible. The perception score, if a direction is picked.
    fn score(&self, ctx: &Ctx) -> f32 {
        if ctx.attr.dodge_threshold > ctx.dodge_clock {
            return 0.0;
        }
        if ctx.me.state == 0 || !ctx.can_enter_dash {
            return 0.0;
        }
        let p = self.perception.score(ctx);
        if p == 0.0 || self.pick(ctx).is_none() {
            return 0.0;
        }
        p
    }

    fn enter(&mut self, ctx: &mut Ctx) {
        self.perception.enter(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) {
        self.perception.leave(ctx);
    }

    /// Start: the game picks the direction in the priority (and stores it in ctrl+0xe4). The port
    /// picks it here from the same draw [stand-in: the game's score is `&self` in this port].
    fn start(&mut self, ctx: &mut Ctx) {
        self.dir = self.pick(ctx).unwrap_or(0);
    }

    /// Update 0081c4d0 [game]: sends the chosen direction to the character (00829df0, unread).
    fn update(&mut self, ctx: &mut Ctx, _dt: f32) {
        ctx.requests.push(Request::Dodge(self.dir));
    }

    /// End 0081c570 [game]: the dodge clock is reset.
    fn stop(&mut self, ctx: &mut Ctx) {
        ctx.dodge_clock = 0.0;
    }

    /// Done 0081c590 [game]: in StateDash and the dash end test passes. The sense of 0074ad50 is
    /// [unknown]; the caller fills `ctx.dash_done`.
    fn finished(&self, ctx: &Ctx) -> bool {
        ctx.me.state == STATE_DASH && ctx.dash_done
    }

    fn interruptible(&self, _ctx: &Ctx) -> bool {
        true
    }
}

/// AIBehaviorInteract (1 object; the goal goalThrowCarsAlways) [game, ai-targeting-idle.md C].
/// Score: 0 in the fall form; else the perception aggregate when something grabbable is registered
/// on the character (`Senses::interact_handle`, character+0x26c); else 0 [game 0081e750]. Update
/// presses button 0 (Action) [game 0081e710]. Finished at once [game 0076c530 family, 1].
/// [stand-in] who sets character+0x26c is NOT FOUND, so the integrator keeps the handle false.
pub struct Interact {
    pub perception: Aggregate,
    /// [unknown] the channel's use (see AttackRanged). Channel 6 from the vtable [game 0081e620].
    pub channel: usize,
}

impl Behaviour for Interact {
    fn channel(&self) -> usize {
        self.channel
    }

    fn score(&self, ctx: &Ctx) -> f32 {
        if ctx.senses.falling_form || !ctx.senses.interact_handle {
            return 0.0;
        }
        self.perception.score(ctx)
    }

    fn enter(&mut self, ctx: &mut Ctx) {
        self.perception.enter(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) {
        self.perception.leave(ctx);
    }

    fn start(&mut self, _ctx: &mut Ctx) {}

    /// Update 0081e710 [game]: button 0 pressed. [stand-in] the game gates it on the AI device
    /// answering vt+4 == 1; this port treats the device as answering (ai-behaviours-move 10e).
    fn update(&mut self, ctx: &mut Ctx, _dt: f32) {
        ctx.requests.push(Request::Button(BTN_ACTION));
    }

    fn stop(&mut self, _ctx: &mut Ctx) {}

    fn finished(&self, _ctx: &Ctx) -> bool {
        true
    }

    fn interruptible(&self, _ctx: &Ctx) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_brain::AiAttr;
    use crate::ai_perceive::{Link, Timer, CALC_AVG};

    /// A perception aggregate that passes (score 1).
    fn pass() -> Aggregate {
        Aggregate::new(
            CALC_AVG,
            0,
            vec![Link {
                invert: false,
                perception: Box::new(Timer::new(1.0, 36.0, true)),
                message: 0,
                arg: 0,
            }],
        )
    }

    /// A perception aggregate that fails (score 0).
    fn fail() -> Aggregate {
        Aggregate::new(
            CALC_AVG,
            0,
            vec![Link {
                invert: false,
                perception: Box::new(Timer::new(1.0, 36.0, false)),
                message: 0,
                arg: 0,
            }],
        )
    }

    /// A context with a foe at `distance` straight ahead (the heading is 0, facing +y).
    fn ctx_at(distance: f32) -> Ctx {
        let mut c = Ctx::new();
        c.target = Some(Target {
            pos: [0.0, distance, 0.0],
            distance,
            team: 2,
        });
        c
    }

    fn ranged(perception: Aggregate) -> AttackRanged {
        AttackRanged {
            ignore_attack_delay: true,
            ignore_kung_fu: false,
            weapon_index: -1,
            perception,
            channel: 0,
        }
    }

    #[test]
    fn ranged_scores_a_foe_in_range_only() {
        let r = ranged(pass());
        assert_eq!(r.score(&ctx_at(4000.0)), 1.0);
        assert_eq!(r.score(&ctx_at(6000.0)), 0.0, "beyond 5000");
        assert_eq!(r.score(&Ctx::new()), 0.0, "no foe");
        assert_eq!(ranged(fail()).score(&ctx_at(4000.0)), 0.0, "perception fails");
        let mut c = ctx_at(4000.0);
        c.weapons_disabled = true;
        assert_eq!(r.score(&c), 0.0, "weapons disabled");
    }

    #[test]
    fn ranged_fires_and_resets_the_clock() {
        let mut r = ranged(pass());
        let mut c = ctx_at(4000.0);
        c.attack_clock = 3.0;
        r.update(&mut c, 0.032);
        assert_eq!(c.requests, vec![Request::WeaponMode(-1), Request::Button(0x17)]);
        assert_eq!(c.attack_clock, 0.0);

        c.requests.clear();
        c.held = 1u64 << 0x17;
        r.update(&mut c, 0.032);
        assert_eq!(c.requests, vec![Request::WeaponMode(-1)], "fire already held: no second press");
    }

    #[test]
    fn ranged_turns_toward_a_foe_outside_the_aim_cone() {
        let mut r = ranged(pass());
        r.weapon_index = 2;
        let mut c = Ctx::new();
        c.target = Some(Target {
            pos: [10.0, 0.0, 0.0],
            distance: 10.0,
            team: 2,
        });
        c.attr = AiAttr {
            aim_test: true,
            aim_cone: 0.3,
            ..AiAttr::default()
        };
        r.update(&mut c, 0.032);
        assert_eq!(c.requests, vec![Request::WeaponMode(2)], "on foot, outside the cone: no fire");

        c.requests.clear();
        c.target = Some(Target {
            pos: [0.0, 100.0, 0.0],
            distance: 100.0,
            team: 2,
        });
        r.update(&mut c, 0.032);
        assert_eq!(
            c.requests,
            vec![Request::WeaponMode(-1), Request::Button(0x17)],
            "straight ahead, inside the cone: fire"
        );
    }

    fn melee(kind: u32, ignore_delay: bool) -> AttackMelee {
        AttackMelee {
            kind,
            ignore_attack_delay: ignore_delay,
            ignore_kung_fu: false,
            perception: pass(),
            channel: 0,
        }
    }

    #[test]
    fn melee_special_is_blocked_by_the_special_float() {
        let m = melee(BTN_ATTACK_SPECIAL, true);
        let mut c = Ctx::new();
        assert_eq!(m.score(&c), 1.0);
        c.me.special_block = 0.5;
        assert_eq!(m.score(&c), 0.0, "a special float above 0 blocks it");
    }

    #[test]
    fn melee_fast_waits_for_its_delay_unless_combo() {
        let m = melee(BTN_ATTACK_FAST, false);
        let mut c = Ctx::new();
        c.attr.attack_delay = 0.5;
        c.attack_clock = 0.2;
        assert_eq!(m.score(&c), 0.0, "the delay has not run");
        c.combo_window = true;
        assert_eq!(m.score(&c), 1.0, "a combo window is open");
        c.combo_window = false;
        c.attack_clock = 0.6;
        assert_eq!(m.score(&c), 1.0, "the delay has run");
    }

    #[test]
    fn melee_presses_outside_attack_state_and_in_combo() {
        let mut m = melee(BTN_ATTACK_FAST, true);
        let mut c = Ctx::new();
        c.me.state = 0x12345678;
        c.attack_clock = 1.0;
        m.update(&mut c, 0.032);
        assert_eq!(c.requests, vec![Request::Button(BTN_ATTACK_FAST)]);
        assert_eq!(c.attack_clock, 0.0);

        c.requests.clear();
        c.me.state = STATE_ATTACK;
        c.attack_clock = 0.7;
        m.update(&mut c, 0.032);
        assert!(c.requests.is_empty(), "in StateAttack without a combo window: no press");
        assert_eq!(c.attack_clock, 0.7, "and no clock reset (0081bc1a skips both)");
        c.combo_window = true;
        m.update(&mut c, 0.032);
        assert_eq!(c.requests, vec![Request::Button(BTN_ATTACK_FAST)], "combo window: press");
    }

    #[test]
    fn transform_needs_the_car_and_presses_its_kind() {
        let mut t = AttackTransform {
            kind: BTN_HOVER,
            ignore_kung_fu: true,
            perception: pass(),
            channel: 0,
        };
        let mut c = Ctx::new();
        assert_eq!(t.score(&c), 0.0, "on foot");
        c.me.in_vehicle = true;
        assert_eq!(t.score(&c), 1.0, "in the car");
        t.update(&mut c, 0.032);
        assert_eq!(c.requests, vec![Request::Button(BTN_HOVER)]);

        c.requests.clear();
        c.held = 1u64 << BTN_HOVER;
        t.update(&mut c, 0.032);
        assert!(c.requests.is_empty(), "already held");

        let bad = AttackTransform {
            kind: BTN_SLAM + 1,
            ignore_kung_fu: true,
            perception: pass(),
            channel: 0,
        };
        assert_eq!(bad.score(&c), 0.0, "a kind outside 0x21..0x24");
    }

    #[test]
    fn weapon_switch_disables_weapons_while_it_runs() {
        let mut w = WeaponSwitch {
            weapon_index: 3,
            disable_weapons: false,
            perception: pass(),
            channel: 0,
        };
        let mut c = Ctx::new();
        c.me.form = 2;
        c.me.weapon_index = 1;
        assert_eq!(w.score(&c), 1.0);
        w.start(&mut c);
        assert!(!c.weapons_disabled);
        w.update(&mut c, 0.032);
        assert_eq!(c.requests, vec![Request::WeaponSwitch(3)]);
        w.stop(&mut c);
        assert!(!c.weapons_disabled);

        c.requests.clear();
        c.me.form = 0;
        w.update(&mut c, 0.032);
        assert!(c.requests.is_empty(), "form 0 and no weapon switch attribute: no switch");

        let mut off = WeaponSwitch {
            weapon_index: -1,
            disable_weapons: true,
            perception: pass(),
            channel: 0,
        };
        off.start(&mut c);
        assert!(c.weapons_disabled, "WeaponsOff sets the flag");
        assert!(off.finished(&c));
        off.stop(&mut c);
        assert!(!c.weapons_disabled);
    }

    #[test]
    fn idle_presses_vehicle_mode_and_never_finishes() {
        let mut i = Idle {
            track_target: false,
            vehicle_mode: true,
            weapon_mode: false,
            perception: pass(),
            channel: 0,
        };
        let mut c = Ctx::new();
        i.update(&mut c, 0.032);
        assert_eq!(c.requests, vec![Request::Button(BTN_VEHICLE_MODE), Request::Release(BTN_WEAPON_MODE)]);
        assert!(!i.finished(&c));
        assert!(i.interruptible(&c));
    }

    #[test]
    fn idle_track_turns_toward_the_target_only_with_auto_facing() {
        let mut i = Idle {
            track_target: true,
            vehicle_mode: false,
            weapon_mode: false,
            perception: pass(),
            channel: 0,
        };
        let mut c = Ctx::new();
        c.target = Some(Target { pos: [3.0, 40.0, 0.0], distance: 40.0, team: 2 });
        i.update(&mut c, 0.032);
        assert_eq!(c.face, None, "auto facing off (the default): no turn");
        c.attr.auto_facing = true;
        i.update(&mut c, 0.032);
        assert_eq!(c.face, Some([3.0, 40.0, 0.0]), "auto facing on: face the target");
        assert!(c.requests.is_empty(), "no button for a plain track");
        c.target = None;
        c.face = None;
        i.update(&mut c, 0.032);
        assert_eq!(c.face, None, "no target: no turn");
    }

    #[test]
    fn interact_scores_only_with_a_handle_and_not_in_the_fall_form() {
        let it = Interact { perception: pass(), channel: 6 };
        let mut c = Ctx::new();
        assert_eq!(it.score(&c), 0.0, "no handle: 0");
        c.senses.interact_handle = true;
        assert_eq!(it.score(&c), 1.0, "handle and an empty perception list: 1");
        c.senses.falling_form = true;
        assert_eq!(it.score(&c), 0.0, "fall form: 0");
        c.senses.falling_form = false;
        let mut i = Interact { perception: pass(), channel: 6 };
        i.update(&mut c, 0.032);
        assert_eq!(c.requests, vec![Request::Button(BTN_ACTION)], "update presses Action");
    }

    #[test]
    fn idle_weapon_mode_requests_and_returns() {
        let mut i = Idle {
            track_target: false,
            vehicle_mode: true,
            weapon_mode: true,
            perception: pass(),
            channel: 0,
        };
        let mut c = Ctx::new();
        i.update(&mut c, 0.032);
        assert_eq!(c.requests, vec![Request::WeaponMode(-1)], "returns before the aim press");
    }

    fn dodge(weights: [f32; 4]) -> Dodge {
        Dodge {
            weights,
            dir: 0,
            perception: pass(),
            channel: 0,
        }
    }

    #[test]
    fn dodge_needs_threshold_state_and_dash() {
        let d = dodge([0.0, 0.0, 100.0, 0.0]);
        let mut c = Ctx::new();
        c.attr.dodge_threshold = 1.0;
        c.me.state = 0x12345678;
        c.can_enter_dash = true;
        c.random = 0.5;
        c.dodge_clock = 0.5;
        assert_eq!(d.score(&c), 0.0, "clock below the threshold");
        c.dodge_clock = 1.0;
        assert_eq!(d.score(&c), 1.0, "threshold met");
        c.can_enter_dash = false;
        assert_eq!(d.score(&c), 0.0, "no dash");
        c.can_enter_dash = true;
        c.me.state = 0;
        assert_eq!(d.score(&c), 0.0, "no current state");
    }

    #[test]
    fn dodge_picks_the_window_and_sends_it() {
        let mut d = dodge([0.0, 0.0, 100.0, 0.0]);
        let mut c = Ctx::new();
        c.random = 0.5;
        d.start(&mut c);
        assert_eq!(d.dir, 2, "forward window: 50 of 100");
        d.update(&mut c, 0.032);
        assert_eq!(c.requests, vec![Request::Dodge(2)]);

        let left_right = dodge([25.0, 25.0, 0.0, 0.0]);
        c.random = 0.3;
        assert_eq!(left_right.pick(&c), Some(0), "15 of 50: left");
        c.random = 0.7;
        assert_eq!(left_right.pick(&c), Some(1), "35 of 50: right");
        assert_eq!(dodge([0.0; 4]).pick(&c), None, "no weight: nothing");
    }

    #[test]
    fn dodge_ends_in_the_dash_and_clears_its_clock() {
        let mut d = dodge([0.0, 0.0, 100.0, 0.0]);
        let mut c = Ctx::new();
        c.me.state = STATE_DASH;
        assert!(!d.finished(&c), "dash end test not passed");
        c.dash_done = true;
        assert!(d.finished(&c));
        c.dodge_clock = 2.0;
        d.stop(&mut c);
        assert_eq!(c.dodge_clock, 0.0);
    }
}
