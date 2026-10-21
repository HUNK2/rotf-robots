//! The reticle: what the game draws round the middle of the picture for his weapons. It is
//! not part of the Flash HUD: the executable draws it itself, textured quads in a picture
//! of 1280 x 720 (`FUN_007b8420` and what it calls; the textures are loaded by path in
//! `FUN_007b54c0` and live in `bnxglobal.str`). [game]
//!
//! Not ported: the arrows towards whoever hurt him (`FUN_007b80b0`, `hitarrow`: nothing
//! here hurts him).

use glam::Vec2;

use crate::lockon::View;
use crate::weapons::Kind;

/// The picture the quads are placed in.
pub const PICTURE: Vec2 = Vec2::new(1280.0, 720.0);
const MIDDLE: Vec2 = Vec2::new(640.0, 360.0);

/// [game: 007b8420] At most one cue per displayed frame, even if several
/// thresholds were crossed. A completed lock takes precedence over progress.
pub fn lock_sound(previous: f32, current: f32) -> Option<u32> {
    if current >= 1.0 && previous < 1.0 {
        Some(crate::formats::hash::crc32(b"UI_LOCKED_ON"))
    } else if [0.25, 0.5, 0.7, 0.85, 0.95].into_iter().any(|p| previous < p && current >= p) {
        Some(crate::formats::hash::crc32(b"UI_LOCK_ON"))
    } else {
        None
    }
}

/// The textures, named as `FUN_007b54c0` names them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Texture {
    BaseGun,
    BaseGunTarget,
    BaseMissile,
    BaseMissileTarget,
    BaseGunCharge,
    BaseGrenade,
    BaseGrenadeCharge,
    BaseGrenadeTarget,
    BaseFlame,
    BaseFlameTarget,
    HeatLeftFrame,
    HeatLeftFill,
    HeatRightFrame,
    HeatRightFill,
    HeatLeftFrameLocked,
    HeatLeftFillLocked,
    HeatRightFrameLocked,
    HeatRightFillLocked,
    LockLine,
    Hit,
    WeaponFrame,
    IconRapid,
    IconShotgun,
    IconCharge,
    IconMissile,
    IconGrenade,
    IconFlame,
    Bracket,
    BracketFocus,
    BaseSniper,
    BaseSniperTarget,
    IconSniper,
    ChargeFill,
    ChargeFrame,
    ChargeFillLocked,
    ChargeFrameLocked,
}

impl Texture {
    pub const ALL: [Texture; 36] = [
        Texture::BaseGun,
        Texture::BaseGunTarget,
        Texture::BaseMissile,
        Texture::BaseMissileTarget,
        Texture::BaseGunCharge,
        Texture::BaseGrenade,
        Texture::BaseGrenadeCharge,
        Texture::BaseGrenadeTarget,
        Texture::BaseFlame,
        Texture::BaseFlameTarget,
        Texture::HeatLeftFrame,
        Texture::HeatLeftFill,
        Texture::HeatRightFrame,
        Texture::HeatRightFill,
        Texture::HeatLeftFrameLocked,
        Texture::HeatLeftFillLocked,
        Texture::HeatRightFrameLocked,
        Texture::HeatRightFillLocked,
        Texture::LockLine,
        Texture::Hit,
        Texture::WeaponFrame,
        Texture::IconRapid,
        Texture::IconShotgun,
        Texture::IconCharge,
        Texture::IconMissile,
        Texture::IconGrenade,
        Texture::IconFlame,
        Texture::Bracket,
        Texture::BracketFocus,
        Texture::BaseSniper,
        Texture::BaseSniperTarget,
        Texture::IconSniper,
        Texture::ChargeFill,
        Texture::ChargeFrame,
        Texture::ChargeFillLocked,
        Texture::ChargeFrameLocked,
    ];

    /// The file's name under `c:\tf2\export\textures\crosshair\`, without `.dds`.
    pub fn file(self) -> &'static str {
        match self {
            Texture::BaseGun => "hud_reticle_base_gun",
            Texture::BaseGunTarget => "hud_reticle_base_gun_target",
            Texture::BaseMissile => "hud_reticle_base_missle",
            Texture::BaseMissileTarget => "hud_reticle_base_missle_target",
            Texture::BaseGunCharge => "hud_reticle_base_gun_chg",
            Texture::BaseGrenade => "hud_reticle_base_msg",
            Texture::BaseGrenadeCharge => "hud_reticle_base_msg_chg",
            Texture::BaseGrenadeTarget => "hud_reticle_base_msg_target",
            Texture::BaseFlame => "hud_reticle_base_flame",
            Texture::BaseFlameTarget => "hud_reticle_base_flame_target",
            Texture::HeatLeftFrame => "hud_reticle_heat_l_frame",
            Texture::HeatLeftFill => "hud_reticle_heat_l_fill",
            Texture::HeatRightFrame => "hud_reticle_heat_r_frame",
            Texture::HeatRightFill => "hud_reticle_heat_r_fill",
            Texture::HeatLeftFrameLocked => "hud_reticle_heat_l_frame_locked",
            Texture::HeatLeftFillLocked => "hud_reticle_heat_l_fill_locked",
            Texture::HeatRightFrameLocked => "hud_reticle_heat_r_frame_locked",
            Texture::HeatRightFillLocked => "hud_reticle_heat_r_fill_locked",
            Texture::LockLine => "hud_reticle_lockline",
            Texture::Hit => "hud_reticle_hit",
            Texture::WeaponFrame => "hud_reticle_weapon_frame_l",
            Texture::IconRapid => "hud_icon_weapon_rapid",
            Texture::IconShotgun => "hud_icon_weapon_shotgun",
            Texture::IconCharge => "hud_icon_weapon_charge",
            Texture::IconMissile => "hud_icon_weapon_missile",
            Texture::IconGrenade => "hud_icon_weapon_grenade",
            Texture::IconFlame => "hud_icon_weapon_flame",
            Texture::Bracket => "crosshair_missile",
            Texture::BracketFocus => "crosshair_missile_focus",
            // `00cf5cf0`, `00cf5d08`, `00cf5d5c` (`reticle_load_textures` 007b54c0). [game]
            Texture::BaseSniper => "hud_reticle_base_sniper",
            Texture::BaseSniperTarget => "hud_reticle_base_sniper_target",
            Texture::IconSniper => "hud_icon_weapon_sniper",
            // The charge bar: `00cf5d0c`, `00cf5d10`, `00cf5d14`, `00cf5d18` (`reticle_load_textures`
            // 007b54c0 l.189-247). The locked fill `d14` is loaded but `reticle_draw_middle` never
            // draws it: the fill is `d0c` on a target too. [game]
            Texture::ChargeFill => "hud_reticle_charge_fill",
            Texture::ChargeFrame => "hud_reticle_charge_frame",
            Texture::ChargeFillLocked => "hud_reticle_charge_fill_locked",
            Texture::ChargeFrameLocked => "hud_reticle_charge_frame_locked",
        }
    }

    /// The texture's id in the pack: the hash of the whole path (`FUN_00550ac0`). [game]
    pub fn id(self) -> u32 {
        crate::formats::hash::lower33(&format!("c:\\tf2\\export\\textures\\crosshair\\{}.dds", self.file()))
    }
}

/// One quad (`FUN_0055f670`): its middle and size in the picture, the part of the texture
/// (left, top, right, bottom, 0 to 1; left over right is mirrored), its opacity and its
/// turn about its middle, clockwise in radians.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sprite {
    pub texture: Texture,
    pub at: Vec2,
    pub size: Vec2,
    pub uv: [f32; 4],
    pub alpha: f32,
    pub turn: f32,
    /// `at` came from a point of the world (the game scales it by 1280 / width and 720 /
    /// height); the others are fixed places.
    pub projected: bool,
}

impl Sprite {
    fn fixed(texture: Texture, at: Vec2, size: Vec2, alpha: f32) -> Self {
        Self { texture, at, size, uv: [0.0, 0.0, 1.0, 1.0], alpha, turn: 0.0, projected: false }
    }
}

/// One of the manager's two weapons (`+0x34`, drawn on the left, and `+0x3c`, on the right).
#[derive(Clone, Copy, Debug)]
pub struct Side {
    pub kind: Kind,
    /// The heat (slot `+0xf0`) and the most it holds (slot `+0xb0`).
    pub heat: f32,
    pub capacity: f32,
    pub charged_shot: bool,
    pub projectiles: u32,
    /// Seconds the weapon's word 0xf6 has counted (`FUN_007a73c0`: up while its slot
    /// `+0x50` says yes, else 0).
    pub shown_for: f32,
    /// The weapon's slot `+0xbc` (0074f150 -> `weapon_snipe_camera_enabled` 0074f210,
    /// attributes `+0x5f`): `snipeCameraEnabled`, which gives the sniper's icon. [game]
    pub sniper: bool,
}

impl Side {
    /// The meter: the heat, at most the capacity, over the capacity.
    fn meter(&self) -> f32 {
        if self.capacity <= 0.0 { 0.0 } else { self.heat.min(self.capacity) / self.capacity }
    }

    fn icon(&self) -> Texture {
        // [game 007b7560 lines 5-30] By the weapon's kind (weapon slot `+0x88`: its attributes'
        // slot `+0x10`, the kind 00784170 also dispatches on: 1 missile, 2 grenade, 4 turret):
        // 0 a gun: the charge with a charged shot (`+0x62`), the shotgun with more than one
        // projectile a firing (`+0x10`), else the rapid one; 1 the missile; 2 the grenade; 3 the
        // flame; any other kind (the turret launcher's 4) keeps the first pick, the rapid one.
        // [game 007b7560 / 007b7f40: kind 3 is the one with the flame's base and icon]
        // [not ported: the "snaker" icon (gun attribute `+0x174` or launcher `+0x1bc` over 0)]
        // After the pick by kind, a weapon with `snipeCameraEnabled` (slot `+0xbc`) takes the
        // sniper's icon `00cf5d5c` instead (007b7560 l.647): the Audi's PlasmaLongRifle and
        // Breakaway's LongBeamRifle. [game] [data]
        if self.sniper {
            return Texture::IconSniper;
        }
        match self.kind {
            Kind::Launcher => Texture::IconMissile,
            Kind::Grenade => Texture::IconGrenade,
            Kind::Flame => Texture::IconFlame,
            Kind::Gun if self.charged_shot => Texture::IconCharge,
            Kind::Gun if self.projectiles > 1 => Texture::IconShotgun,
            Kind::Gun | Kind::Turret => Texture::IconRapid,
        }
    }
}

/// The bracket round the target a launcher is locking on or has locked.
#[derive(Clone, Copy, Debug)]
pub struct Bracket {
    pub at: Vec2,
    pub size: f32,
    pub locked: bool,
}

/// The bracket for a target standing at `place`, `height` tall and `half_width` to each
/// side: its middle is the target's, its size 1.5 times the larger of the height and the
/// width as the picture shows them, between 30 and 300 (`00b8d9c8`...). [game]
pub fn bracket(view: &View, place: glam::Vec3, height: f32, half_width: f32, locked: bool) -> Option<Bracket> {
    let at = view.project(place + glam::Vec3::Z * (height * 0.5))?;
    let low = view.project(place)?;
    let high = view.project(place + glam::Vec3::Z * height)?;
    let left = view.project(place - view.right * half_width)?;
    let right = view.project(place + view.right * half_width)?;
    let size = ((low.y - high.y).abs().max((right.x - left.x).abs()) * 1.5).clamp(30.0, 300.0);
    Some(Bracket { at, size, locked })
}

/// What one frame's drawing is made from.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    /// His control mode is the weapon one (`+0x31c` == 2, or `FUN_00721b50`).
    pub weapon_mode: bool,
    pub primary: Option<Side>,
    pub secondary: Option<Side>,
    /// The weapon in hand (`FUN_007b5030(manager, 0)`), which picks the base.
    pub current: Option<Kind>,
    /// The weapon in hand has a charged shot (its slot `+0x124` not negative), read from the
    /// weapon at the manager's current index whatever its place in the list [game 007b7f40,
    /// 007b5030]. None only in frames built without it (tests): the side of its kind stands in.
    pub hand_charged: Option<bool>,
    /// The weapon in hand has `snipeCameraEnabled` (its slot `+0xbc`): the sniper's bases,
    /// whether he is sniping or not. [game 007b7f40]
    pub current_sniper: bool,
    /// The aim assist has a target within the weapon's range: the `_target` textures.
    pub on_target: bool,
    /// The lock, 0 to 1 (`FUN_007b2240`; 1 once locked).
    pub lock: f32,
    pub bracket: Option<Bracket>,
    /// Seconds since `+0x234`'s `+0x30`. [assumed: the time one of his shots last hit; what writes it is not read] settles: the writers of `+0x234` offset 0x30 (grep the stores in the weapon hit code, raw_disasm over the callers of 007832b0).
    pub since_hit: f32,
    /// The car's weapon is in hand (`FUN_007b44e0`): the points 30 and 50 ahead of it in
    /// the picture, where the two bases go.
    pub car: Option<(Option<Vec2>, Option<Vec2>)>,
    /// The charge bar's value (`charge_value`): drawn in weapon mode when it is 0 or more.
    pub charge: Option<f32>,
}

/// The charge bar's value, as `hud_draw_reticle` 007b8420 l.709-718 asks the weapon in hand
/// for it: its slot `+0x128` (`weapon_sniper_steady` 007ac920) while `camera_is_sniping`
/// 0117e860, else its slot `+0x124` (`Weapon::charge_level`, 007b0b00: -1 with no charged
/// shot). `reticle_draw_middle` 007b67c0 l.108 draws the bar when it is 0 or more, so while
/// sniping the steady always shows. [game]
pub fn charge_value(sniping: bool, charge_level: f32, steady: f32) -> Option<f32> {
    Some(if sniping { steady } else { charge_level }).filter(|v| *v >= 0.0)
}

/// The reticle's own state: the two meters' flashes (`00d4fdd8`, `00d4fddc`), the charge
/// bar's (`00d4fdd4`) and the bracket's (`00cf5c9f`, `00cf5d6c`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Reticle {
    flash: [f32; 2],
    charge_flash: f32,
    focus: bool,
    focus_wait: f32,
}

/// The hit mark's strength: whole for 0.225 s, gone at 0.45. Drawn squared. [game]
pub fn hit_strength(since: f32) -> f32 {
    if !(0.0..=0.45).contains(&since) {
        0.0
    } else if since > 0.225 {
        1.0 - (since - 0.225) * 4.444_444_7
    } else {
        1.0
    }
}

impl Reticle {
    /// A full meter's fill flashes: the square root of 1 - 2 t, t running to 0.5 and
    /// starting again (`FUN_007b6f90`). [game]
    fn fill_alpha(clock: &mut f32, meter: f32, dt: f32) -> f32 {
        if meter != 1.0 {
            *clock = 0.0;
            return 1.0;
        }
        let alpha = (1.0 - *clock * 2.0).clamp(0.0, 1.0).sqrt();
        *clock += dt;
        if *clock >= 0.5 {
            *clock = 0.0;
        }
        alpha
    }

    /// A weapon's icon and its frame (`FUN_007b7560`): they come up beside the middle as
    /// the weapon's clock starts, and after 1.5 s shrink into a small frame that stays in
    /// weapon mode and is gone out of it. `side` is -1 for the left, 1 for the right.
    fn icon(out: &mut Vec<Sprite>, weapon: &Side, side: f32, weapon_mode: bool) {
        let t = weapon.shown_for;
        if t <= 0.0 {
            return;
        }
        let frame_alpha = if t < 0.2 { t * 5.0 } else { 1.0 };
        // 1 from 0.5 s on: the icon's alpha starts at 1.0 (`param_1 = 0x3f800000`) and only
        // under 0.5 s is it (t - 0.3) * 5. [game 007b7560]
        let mut icon_alpha = if t < 0.5 { (t - 0.3) * 5.0 } else { 1.0 };
        let (mut scale, mut away) = (1.0, 90.0);
        if t > 1.5 {
            if t >= 1.7 {
                (scale, away, icon_alpha) = (0.3, 74.0, 0.0);
            } else {
                let k = (t - 1.5) * 5.0;
                (scale, away, icon_alpha) = (1.0 - k * 0.7, 90.0 - k * 16.0, 1.0 - k);
            }
        }
        let uv = if side < 0.0 { [0.0, 0.0, 1.0, 1.0] } else { [1.0, 0.0, 0.0, 1.0] };
        if t < 1.5 || weapon_mode {
            let at = MIDDLE + Vec2::new(side * away, 0.0);
            out.push(Sprite { uv, ..Sprite::fixed(Texture::WeaponFrame, at, Vec2::splat(scale * 52.0), frame_alpha) });
            if icon_alpha > 0.0 {
                out.push(Sprite::fixed(weapon.icon(), at, Vec2::splat(scale * 48.0), icon_alpha));
            }
        } else if icon_alpha > 0.0 {
            let at = MIDDLE + Vec2::new(side * 90.0, 0.0);
            out.push(Sprite { uv, ..Sprite::fixed(Texture::WeaponFrame, at, Vec2::splat(52.0), icon_alpha) });
            out.push(Sprite::fixed(weapon.icon(), at, Vec2::splat(48.0), icon_alpha));
        }
    }

    /// One frame's quads, in the order the game draws them.
    pub fn draw(&mut self, f: &Frame, dt: f32) -> Vec<Sprite> {
        let mut out = Vec::new();
        let left = f.primary.as_ref().map_or(0.0, Side::meter);
        let right = f.secondary.as_ref().map_or(0.0, Side::meter);
        if let Some(weapon) = &f.primary {
            Self::icon(&mut out, weapon, -1.0, f.weapon_mode);
        }
        if let Some(weapon) = &f.secondary {
            Self::icon(&mut out, weapon, 1.0, f.weapon_mode);
        }
        // Out of weapon mode the meters stay while there is heat. [game, from the
        // pseudocode alone: their opacity there is the last weapon's heat, not its share]
        let alpha = if f.weapon_mode {
            1.0
        } else {
            f.secondary.or(f.primary).map_or(1.0, |w| w.heat.min(w.capacity)).clamp(0.0, 1.0)
        };
        let mut fills = [1.0, 1.0];
        if f.weapon_mode || alpha > 0.0 {
            fills = [Self::fill_alpha(&mut self.flash[0], left, dt) * alpha, Self::fill_alpha(&mut self.flash[1], right, dt) * alpha];
        }
        // [game 007b7f40] The base by the weapon in hand's kind: 0 the gun's (`_chg` when its
        // slot `+0x124` is not negative: 007b0b00 gives -1 exactly when the attributes have no
        // charged shot, `+0x62`), 1 the missile's, 2 the grenade's (`base_msg`, `_chg` likewise),
        // 3 the flame's, any other the gun's; each with its `_target` twin. Then a weapon in
        // hand with `snipeCameraEnabled` (slot `+0xbc`) takes the sniper's pair, `00cf5cf0` and
        // `00cf5d08`, over whatever the kind gave. [game 007b7f40]
        // The game picks the reticle's base by the weapon IN HAND (`reticle_pick_base` 007b7f40:
        // `manager_weapon_in_hand` of the character's `+0x238`, then its kind `+0x88`, charge
        // `+0x124`, sniper `+0xbc`). [game 007b7f40] The charge test reads the weapon IN HAND:
        // `manager_weapon_in_hand` 007b5030 is the manager's current index `+0x74` (the slot there must
        // say yes at `+0x30`), with no first-two-slots rule, and 007b7f40 uses it unchecked [game; wave 29
        // Q8]. The caller passes that weapon's charge (`hand_charged`); a frame without it (tests) takes
        // the side whose clock runs, else the first of its kind.
        let of_kind = || [f.primary, f.secondary].into_iter().flatten().filter(|s| Some(s.kind) == f.current);
        let charged = f.hand_charged.unwrap_or_else(|| of_kind().find(|s| s.shown_for > 0.0).or_else(|| of_kind().next()).is_some_and(|s| s.charged_shot));
        let (base, base_target) = match f.current {
            Some(Kind::Launcher) => (Texture::BaseMissile, Texture::BaseMissileTarget),
            Some(Kind::Grenade) => (if charged { Texture::BaseGrenadeCharge } else { Texture::BaseGrenade }, Texture::BaseGrenadeTarget),
            Some(Kind::Flame) => (Texture::BaseFlame, Texture::BaseFlameTarget),
            Some(Kind::Gun) if charged => (Texture::BaseGunCharge, Texture::BaseGunTarget),
            _ => (Texture::BaseGun, Texture::BaseGunTarget),
        };
        let (base, base_target) = if f.current_sniper { (Texture::BaseSniper, Texture::BaseSniperTarget) } else { (base, base_target) };
        let hit = hit_strength(f.since_hit);
        if f.weapon_mode {
            if let Some(bracket) = f.bracket {
                // While it is being locked the bracket changes picture every 0.1 s.
                self.focus_wait -= dt;
                if self.focus_wait < 0.0 {
                    self.focus = !self.focus;
                    self.focus_wait = 0.1;
                }
                let texture = if bracket.locked || self.focus { Texture::BracketFocus } else { Texture::Bracket };
                out.push(Sprite { projected: true, ..Sprite::fixed(texture, bracket.at, Vec2::splat(bracket.size), 1.0) });
            }
            out.push(Sprite::fixed(if f.on_target { base_target } else { base }, MIDDLE, Vec2::splat(128.0), 1.0));
            // Four lines that grow towards the middle with the lock (`FUN_007b67c0`).
            let p = f.lock;
            let size = Vec2::new(16.0, p * 48.0 + 16.0);
            let uv = [0.0, 0.0, 1.0, p * 0.75 + 0.25];
            let reach = p * 24.0;
            for (at, turn) in [
                (Vec2::new(640.0, reach + 304.0), 0.0),
                (Vec2::new(640.0, 424.0 - (reach + 8.0)), std::f32::consts::PI),
                (Vec2::new(704.0 - (reach + 8.0), 360.0), std::f32::consts::FRAC_PI_2),
                (Vec2::new(reach + 584.0, 360.0), -std::f32::consts::FRAC_PI_2),
            ] {
                out.push(Sprite { uv, turn, ..Sprite::fixed(Texture::LockLine, at, size, 1.0) });
            }
            if hit > 0.0 {
                out.push(Sprite::fixed(Texture::Hit, MIDDLE, Vec2::splat(128.0), hit * hit));
            }
            if let Some(v) = f.charge.filter(|v| *v >= 0.0) {
                // The charge bar (`reticle_draw_middle` 007b67c0 l.108-174), under the middle: the
                // frame (`d10`, or `d18` on a target) 64 x 32 at (640, 424) in the reticle's
                // opacity (1 in weapon mode, the only mode that reaches it: 007b8420 l.101-119),
                // then the fill `d0c`, `v` x 64 wide and 32 high at the same middle, showing the
                // texture's middle part (u from 0.5 - v / 2 to 0.5 + v / 2): it grows out from the
                // middle both ways. Its opacity is 1 below full; from 1 on it flashes as the
                // meters do, on its own clock `00d4fdd4` (`sqrt(clamp(1 - 2 t))`, t then on by the
                // frame's step and back to 0 at 0.5 s; 0 again whenever the value is under 1).
                // [game] The clock only moves while the bar is drawn, as in the game.
                let at = Vec2::new(640.0, 424.0);
                let frame = if f.on_target { Texture::ChargeFrameLocked } else { Texture::ChargeFrame };
                out.push(Sprite::fixed(frame, at, Vec2::new(64.0, 32.0), 1.0));
                let mut fill = 1.0;
                if v < 1.0 {
                    self.charge_flash = 0.0;
                } else {
                    fill = (1.0 - self.charge_flash * 2.0).clamp(0.0, 1.0).sqrt();
                    self.charge_flash += dt;
                    if self.charge_flash >= 0.5 {
                        self.charge_flash = 0.0;
                    }
                }
                // A fill 0 wide is left out: the game draws it, and nothing shows.
                if v > 0.0 {
                    out.push(Sprite {
                        uv: [0.5 - v * 0.5, 0.0, v * 0.5 + 0.5, 1.0],
                        ..Sprite::fixed(Texture::ChargeFill, at, Vec2::new(v * 64.0, 32.0), fill)
                    });
                }
            }
        } else if let Some((near, far)) = f.car {
            // The car's gun: a small base 50 ahead of it, the full one 30 ahead.
            if let Some(far) = far {
                out.push(Sprite { projected: true, ..Sprite::fixed(base, far, Vec2::splat(77.0), 1.0) });
            }
            if let Some(near) = near {
                out.push(Sprite { projected: true, ..Sprite::fixed(if f.on_target { base_target } else { base }, near, Vec2::splat(128.0), 1.0) });
                if hit > 0.0 {
                    out.push(Sprite { projected: true, ..Sprite::fixed(Texture::Hit, near, Vec2::splat(128.0), hit * hit) });
                }
            }
        }
        if alpha > 0.0 {
            // The two meters (`FUN_007b7170`): frames either side, fills from the bottom up.
            let locked = f.on_target;
            let pick = |plain, locked_one| if locked { locked_one } else { plain };
            out.push(Sprite::fixed(pick(Texture::HeatLeftFrame, Texture::HeatLeftFrameLocked), Vec2::new(576.0, 360.0), Vec2::new(32.0, 64.0), alpha));
            out.push(Sprite::fixed(pick(Texture::HeatRightFrame, Texture::HeatRightFrameLocked), Vec2::new(704.0, 360.0), Vec2::new(32.0, 64.0), alpha));
            for (meter, x, texture, fill) in [
                (left, 576.0, pick(Texture::HeatLeftFill, Texture::HeatLeftFillLocked), fills[0]),
                (right, 704.0, pick(Texture::HeatRightFill, Texture::HeatRightFillLocked), fills[1]),
            ] {
                if meter > 0.0 {
                    out.push(Sprite {
                        uv: [0.0, 1.0 - meter, 1.0, 1.0],
                        ..Sprite::fixed(texture, Vec2::new(x, 392.0 - meter * 32.0), Vec2::new(32.0, meter * 64.0), fill)
                    });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_cues_cross_once_and_completion_wins_over_skipped_thresholds() {
        for threshold in [0.25, 0.5, 0.7, 0.85, 0.95] {
            assert_eq!(lock_sound(threshold - 0.01, threshold), Some(crate::formats::hash::crc32(b"UI_LOCK_ON")));
            assert_eq!(lock_sound(threshold, threshold), None);
            assert_eq!(lock_sound(threshold, 0.0), None);
        }
        assert_eq!(lock_sound(0.0, 0.96), Some(crate::formats::hash::crc32(b"UI_LOCK_ON")));
        assert_eq!(lock_sound(0.0, 1.0), Some(crate::formats::hash::crc32(b"UI_LOCKED_ON")));
        assert_eq!(lock_sound(1.0, 1.0), None);
    }

    fn gun(heat: f32, shown_for: f32) -> Side {
        Side { kind: Kind::Gun, heat, capacity: 10.0, charged_shot: false, projectiles: 1, shown_for, sniper: false }
    }

    /// [game 007b7560 l.647, 007b7f40] `snipeCameraEnabled` overrides the kind's icon and base.
    #[test]
    fn a_snipe_weapon_has_the_sniper_icon_and_base() {
        let rifle = Side { sniper: true, charged_shot: true, ..gun(0.0, 1.0) };
        assert_eq!(rifle.icon(), Texture::IconSniper);
        for (kind, on_target, base) in [
            (Kind::Gun, false, Texture::BaseSniper),
            (Kind::Gun, true, Texture::BaseSniperTarget),
            (Kind::Grenade, false, Texture::BaseSniper),
        ] {
            let side = Side { kind, ..rifle };
            let frame = Frame { weapon_mode: true, primary: Some(side), current: Some(kind), current_sniper: true, on_target, since_hit: 100.0, ..Default::default() };
            let sprites = Reticle::default().draw(&frame, 0.016);
            assert_eq!(find(&sprites, base).len(), 1, "{kind:?} {on_target}");
            assert_eq!(find(&sprites, Texture::IconSniper).len(), 1);
        }
    }

    fn find(sprites: &[Sprite], texture: Texture) -> Vec<Sprite> {
        sprites.iter().copied().filter(|s| s.texture == texture).collect()
    }

    #[test]
    fn weapon_mode_draws_the_base_the_lines_and_the_meters() {
        let frame = Frame { weapon_mode: true, primary: Some(gun(5.0, 5.0)), current: Some(Kind::Gun), since_hit: 100.0, ..Default::default() };
        let sprites = Reticle::default().draw(&frame, 0.016);
        let base = find(&sprites, Texture::BaseGun);
        assert_eq!((base[0].at, base[0].size), (Vec2::new(640.0, 360.0), Vec2::splat(128.0)));
        // With no lock the lines are 16 square, a quarter of the texture, 56 from the middle.
        let lines = find(&sprites, Texture::LockLine);
        assert_eq!(lines.len(), 4);
        assert_eq!((lines[0].at, lines[0].size, lines[0].uv[3]), (Vec2::new(640.0, 304.0), Vec2::splat(16.0), 0.25));
        assert_eq!(lines[1].at, Vec2::new(640.0, 416.0));
        assert_eq!(lines[2].at, Vec2::new(696.0, 360.0));
        assert_eq!(lines[3].at, Vec2::new(584.0, 360.0));
        // Half a meter fills the lower half of the left frame.
        let fill = find(&sprites, Texture::HeatLeftFill);
        assert_eq!((fill[0].at, fill[0].size, fill[0].uv), (Vec2::new(576.0, 376.0), Vec2::new(32.0, 32.0), [0.0, 0.5, 1.0, 1.0]));
        assert!(find(&sprites, Texture::HeatRightFill).is_empty());
        assert!(find(&sprites, Texture::Hit).is_empty());
        // Past 1.7 s the weapon's frame is the small one, 74 from the middle, no icon.
        let frames = find(&sprites, Texture::WeaponFrame);
        assert_eq!((frames[0].at, frames[0].size), (Vec2::new(566.0, 360.0), Vec2::splat(0.3 * 52.0)));
        assert!(find(&sprites, Texture::IconRapid).is_empty());
    }

    /// [game 007b7560, 007b7f40, 007b0b00] Each kind's icon (seen at 1 s) and base.
    #[test]
    fn each_kind_has_the_games_icon_and_base() {
        let cases = [
            (Side { kind: Kind::Gun, ..gun(0.0, 1.0) }, Texture::IconRapid, Texture::BaseGun),
            (Side { kind: Kind::Gun, charged_shot: true, ..gun(0.0, 1.0) }, Texture::IconCharge, Texture::BaseGunCharge),
            (Side { kind: Kind::Gun, projectiles: 3, ..gun(0.0, 1.0) }, Texture::IconShotgun, Texture::BaseGun),
            (Side { kind: Kind::Launcher, ..gun(0.0, 1.0) }, Texture::IconMissile, Texture::BaseMissile),
            (Side { kind: Kind::Grenade, ..gun(0.0, 1.0) }, Texture::IconGrenade, Texture::BaseGrenade),
            (Side { kind: Kind::Grenade, charged_shot: true, ..gun(0.0, 1.0) }, Texture::IconGrenade, Texture::BaseGrenadeCharge),
            (Side { kind: Kind::Flame, ..gun(0.0, 1.0) }, Texture::IconFlame, Texture::BaseFlame),
            (Side { kind: Kind::Turret, ..gun(0.0, 1.0) }, Texture::IconRapid, Texture::BaseGun),
        ];
        for (side, icon, base) in cases {
            let frame = Frame { weapon_mode: true, primary: Some(side), current: Some(side.kind), since_hit: 100.0, ..Default::default() };
            let sprites = Reticle::default().draw(&frame, 0.016);
            let icons = find(&sprites, icon);
            assert_eq!(icons.len(), 1, "{:?}", side.kind);
            assert_eq!(icons[0].alpha, 1.0, "full between 0.5 and 1.5 s");
            assert_eq!(find(&sprites, base).len(), 1, "{:?}", side.kind);
        }
    }

    /// [game 007b7f40] The base's charge comes from the side IN HAND, not from the clocks: two
    /// guns, the secondary charged and its clock stopped (a switch), the primary in hand.
    #[test]
    fn the_base_takes_its_charge_from_the_side_in_hand() {
        let plain = gun(0.0, 1.0);
        let charged = Side { charged_shot: true, ..gun(0.0, 0.0) };
        let frame = |hand_charged| Frame {
            weapon_mode: true,
            primary: Some(plain),
            secondary: Some(charged),
            current: Some(Kind::Gun),
            hand_charged,
            since_hit: 100.0,
            ..Default::default()
        };
        let sprites = Reticle::default().draw(&frame(Some(plain.charged_shot)), 0.016);
        assert_eq!(find(&sprites, Texture::BaseGun).len(), 1, "the primary in hand is plain");
        let sprites = Reticle::default().draw(&frame(Some(charged.charged_shot)), 0.016);
        assert_eq!(find(&sprites, Texture::BaseGunCharge).len(), 1, "the secondary in hand is charged");
    }

    #[test]
    fn a_lock_grows_the_lines_and_a_target_changes_the_textures() {
        let launcher = Side { kind: Kind::Launcher, ..gun(0.0, 5.0) };
        let frame = Frame {
            weapon_mode: true,
            primary: Some(gun(0.0, 5.0)),
            secondary: Some(launcher),
            current: Some(Kind::Launcher),
            on_target: true,
            lock: 1.0,
            bracket: Some(Bracket { at: Vec2::new(700.0, 300.0), size: 90.0, locked: true }),
            since_hit: 0.3,
            ..Default::default()
        };
        let sprites = Reticle::default().draw(&frame, 0.016);
        assert_eq!(find(&sprites, Texture::BaseMissileTarget).len(), 1);
        let lines = find(&sprites, Texture::LockLine);
        assert_eq!((lines[0].at, lines[0].size, lines[0].uv[3]), (Vec2::new(640.0, 328.0), Vec2::new(16.0, 64.0), 1.0));
        let bracket = find(&sprites, Texture::BracketFocus);
        assert!(bracket[0].projected && bracket[0].size == Vec2::splat(90.0));
        assert_eq!(find(&sprites, Texture::HeatLeftFrameLocked).len(), 1);
        // 0.3 s after a hit the mark is two thirds gone, and drawn squared.
        let hit = find(&sprites, Texture::Hit);
        let strength = 1.0 - 0.075 * 4.444_444_7;
        assert!((hit[0].alpha - strength * strength).abs() < 1e-5);
    }

    #[test]
    fn a_full_meter_flashes_and_out_of_weapon_mode_only_heat_shows() {
        let mut reticle = Reticle::default();
        let frame = Frame { weapon_mode: true, primary: Some(gun(10.0, 5.0)), current: Some(Kind::Gun), since_hit: 100.0, ..Default::default() };
        let alphas: Vec<f32> = (0..6).map(|_| find(&reticle.draw(&frame, 0.1), Texture::HeatLeftFill)[0].alpha).collect();
        assert!((alphas[0] - 1.0).abs() < 1e-6 && (alphas[2] - 0.6f32.sqrt()).abs() < 1e-5);
        assert!((alphas[5] - 1.0).abs() < 1e-6, "the flash starts again at 0.5 s: {alphas:?}");
        // On foot with a cold gun nothing is drawn; with heat, the meters alone.
        let cold = Frame { primary: Some(gun(0.0, 5.0)), since_hit: 100.0, ..Default::default() };
        assert!(Reticle::default().draw(&cold, 0.016).is_empty());
        let warm = Frame { primary: Some(gun(4.0, 5.0)), since_hit: 100.0, ..Default::default() };
        let sprites = Reticle::default().draw(&warm, 0.016);
        assert_eq!(sprites.len(), 3);
        assert!(find(&sprites, Texture::BaseGun).is_empty());
    }

    #[test]
    fn an_icon_comes_up_then_shrinks_into_its_frame() {
        let at = |t: f32, weapon_mode| {
            let frame = Frame { weapon_mode, secondary: Some(Side { kind: Kind::Launcher, ..gun(0.0, t) }), since_hit: 100.0, ..Default::default() };
            let sprites = Reticle::default().draw(&frame, 0.016);
            (find(&sprites, Texture::WeaponFrame), find(&sprites, Texture::IconMissile))
        };
        let (frame, icon) = at(0.1, false);
        assert!((frame[0].alpha - 0.5).abs() < 1e-6 && icon.is_empty());
        // The right one's frame is the left one's mirrored.
        assert_eq!((frame[0].at, frame[0].uv), (Vec2::new(730.0, 360.0), [1.0, 0.0, 0.0, 1.0]));
        let (frame, icon) = at(1.0, false);
        assert_eq!((frame[0].size, icon[0].size, icon[0].alpha), (Vec2::splat(52.0), Vec2::splat(48.0), 1.0));
        let (frame, icon) = at(1.6, true);
        assert!((frame[0].at.x - 722.0).abs() < 1e-4 && (icon[0].alpha - 0.5).abs() < 1e-5);
        let (frame, icon) = at(2.0, false);
        assert!(frame.is_empty() && icon.is_empty());
    }

    #[test]
    fn the_car_has_two_bases_ahead_of_it() {
        let frame = Frame {
            primary: Some(gun(0.0, 0.0)),
            current: Some(Kind::Gun),
            since_hit: 100.0,
            car: Some((Some(Vec2::new(600.0, 380.0)), Some(Vec2::new(610.0, 370.0)))),
            ..Default::default()
        };
        let sprites = Reticle::default().draw(&frame, 0.016);
        let bases = find(&sprites, Texture::BaseGun);
        assert_eq!((bases[0].size, bases[1].size), (Vec2::splat(77.0), Vec2::splat(128.0)));
        assert!(bases.iter().all(|s| s.projected));
    }

    #[test]
    fn a_bracket_is_sized_by_the_target_in_the_picture() {
        let view = View::looking(glam::Vec3::ZERO, glam::Vec3::Y, 60.0, 16.0 / 9.0);
        // 4 tall at 20 away: 4 / (20 tan 30) of half the picture's height, times 1.5.
        let b = bracket(&view, glam::Vec3::new(0.0, 20.0, -2.0), 4.0, 1.0, false).unwrap();
        let tall = 4.0 / (20.0 * 30f32.to_radians().tan()) * 360.0;
        assert!((b.size - tall * 1.5).abs() < 1e-2, "{}", b.size);
        assert!((b.at - Vec2::new(640.0, 360.0)).length() < 1e-3);
        // Far away it is never under 30.
        assert_eq!(bracket(&view, glam::Vec3::new(0.0, 2000.0, 0.0), 4.0, 1.0, true).unwrap().size, 30.0);
    }

    #[test]
    fn the_ids_are_the_hashes_of_the_paths() {
        assert_eq!(Texture::BaseGun.id(), 0x1e43_a6fd);
    }

    /// [game 007b8420 l.709-718, 007b67c0 l.108] The steady while sniping, else the charge level;
    /// a weapon with no charged shot (-1) has no bar.
    #[test]
    fn the_charge_bar_shows_the_steady_while_sniping_else_the_charge() {
        assert_eq!(charge_value(false, 0.4, 0.9), Some(0.4));
        assert_eq!(charge_value(true, 0.4, 0.9), Some(0.9));
        assert_eq!(charge_value(true, -1.0, 0.0), Some(0.0), "a sniper with no charged shot still has the bar");
        assert_eq!(charge_value(false, -1.0, 0.9), None);
    }

    /// [game 007b67c0 l.108-174] The frame 64 x 32 at (640, 424), the fill growing out of its
    /// middle; the locked frame on a target, the plain fill always; only in weapon mode.
    #[test]
    fn the_charge_bar_grows_from_the_middle_under_the_base() {
        let charged = Side { charged_shot: true, ..gun(0.0, 5.0) };
        let frame = Frame { weapon_mode: true, primary: Some(charged), current: Some(Kind::Gun), since_hit: 100.0, charge: Some(0.5), ..Default::default() };
        let sprites = Reticle::default().draw(&frame, 0.016);
        let bar = find(&sprites, Texture::ChargeFrame);
        assert_eq!((bar[0].at, bar[0].size, bar[0].alpha), (Vec2::new(640.0, 424.0), Vec2::new(64.0, 32.0), 1.0));
        let fill = find(&sprites, Texture::ChargeFill);
        assert_eq!((fill[0].at, fill[0].size, fill[0].uv, fill[0].alpha), (Vec2::new(640.0, 424.0), Vec2::new(32.0, 32.0), [0.25, 0.0, 0.75, 1.0], 1.0));
        // The fill is drawn after its frame, and both after the base.
        let order = |t| sprites.iter().position(|s| s.texture == t).unwrap();
        assert!(order(Texture::BaseGunCharge) < order(Texture::ChargeFrame) && order(Texture::ChargeFrame) < order(Texture::ChargeFill));
        let target = Frame { on_target: true, ..frame };
        let sprites = Reticle::default().draw(&target, 0.016);
        assert_eq!((find(&sprites, Texture::ChargeFrameLocked).len(), find(&sprites, Texture::ChargeFill).len()), (1, 1));
        assert!(find(&sprites, Texture::ChargeFillLocked).is_empty() && find(&sprites, Texture::ChargeFrame).is_empty());
        // At 0 the frame alone; with no value, or out of weapon mode, nothing.
        let empty = Reticle::default().draw(&Frame { charge: Some(0.0), ..frame }, 0.016);
        assert_eq!((find(&empty, Texture::ChargeFrame).len(), find(&empty, Texture::ChargeFill).len()), (1, 0));
        for f in [Frame { charge: None, ..frame }, Frame { charge: Some(-1.0), ..frame }, Frame { weapon_mode: false, ..frame }] {
            let sprites = Reticle::default().draw(&f, 0.016);
            assert!(find(&sprites, Texture::ChargeFrame).is_empty() && find(&sprites, Texture::ChargeFill).is_empty());
        }
    }

    /// [game 007b67c0 l.139-159] A full bar's fill flashes on its own clock: sqrt(1 - 2 t), back
    /// to 0 at 0.5 s, and to 0 whenever the value drops under 1.
    #[test]
    fn a_full_charge_bar_flashes_and_starts_again_below_full() {
        let mut reticle = Reticle::default();
        let frame = Frame { weapon_mode: true, primary: Some(gun(0.0, 5.0)), current: Some(Kind::Gun), since_hit: 100.0, charge: Some(1.0), ..Default::default() };
        let alphas: Vec<f32> = (0..7).map(|_| find(&reticle.draw(&frame, 0.1), Texture::ChargeFill)[0].alpha).collect();
        for (got, t) in alphas.iter().zip([0.0f32, 0.1, 0.2, 0.3, 0.4, 0.0, 0.1]) {
            assert!((got - (1.0 - 2.0 * t).max(0.0).sqrt()).abs() < 1e-5, "{alphas:?}");
        }
        // Under full the fill is whole and the clock goes back to 0.
        assert_eq!(find(&reticle.draw(&Frame { charge: Some(0.9), ..frame }, 0.1), Texture::ChargeFill)[0].alpha, 1.0);
        assert_eq!(find(&reticle.draw(&frame, 0.1), Texture::ChargeFill)[0].alpha, 1.0);
    }

    /// [data] Every texture the reticle names is in the install's `bnxglobal.str`.
    #[test]
    fn every_reticle_texture_is_in_the_global_pack() {
        use crate::formats::{pack, texture};
        let Ok(pack) = pack::Pack::open(std::path::Path::new(r"C:\Games2\bnxglobal.str")) else { return };
        let ids: std::collections::HashSet<u32> = pack.of_type(pack::TEXTURE)
            .filter_map(|chunk| texture::parse(pack.data(chunk)).ok().flatten().map(|t| t.id))
            .collect();
        assert!(ids.len() >= Texture::ALL.len(), "{} textures read", ids.len());
        let missing: Vec<_> = Texture::ALL.into_iter().filter(|t| !ids.contains(&t.id())).collect();
        assert!(missing.is_empty(), "not in bnxglobal.str: {missing:?}");
    }
}
