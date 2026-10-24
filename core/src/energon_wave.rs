//! Grindor's cinematic energon wave [port design]: every shot of his energon cannon
//! (`BlackoutEnergonCannon`) opens a wide arc of Devastator's ground ring where its missile ends,
//! instead of only the narrow ground wave of the original. Not in the game: the user's design
//! beyond it (2026-10-24, `notes\grindor-cinematic-wave.md`), built only from the game's own
//! data -- Devastator's `ShockWave` (its material, band, fades, segments, knock-back) and
//! Grindor's own `snakerInfo` (speed, life, damage).
//!
//! The ring is the game's (`shockwave.rs`); what this adds is an `Arc` and a `Falloff` on it,
//! and Devastator's ring scaled from his 157 m run to the snaker's own run.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::formats::characterlist;
use crate::formats::hash::crc32;
use crate::formats::lxb::{DataFile, Node};
use crate::formats::model::{Library, ParamValue};
use crate::formats::pack::{self, Pack};
use crate::particles::Template;
use crate::camera::Sight;
use crate::collision_masks::PUSH;
use crate::shockwave::{Arc, Candidate, Def, Falloff, GROUND_LIFT, PROBE_UP, Ring, ShockWave, Vertex};
use crate::weapons::SnakerTuning;
use glam::{Vec2, Vec3};

/// `BlackoutEnergonCannon`: the weapon whose shots open the wave (Grindor's robot-form cannon;
/// Jetfire's energon cannon keeps the original's narrow wave). [data]
pub const CANNON: u32 = crc32(b"BlackoutEnergonCannon");

/// [port design] The game's sounds the wave adds, every one from the install: the shared level
/// bank's `EXP_HUGE` (heard to 500 m, [data us_city_00 mission.fev], loaded for everyone) layered
/// on the wave's own `WPN_BLAC_CAN_IMP`; Devastator's pound `ANIM_DEVA_POUND` (heard to 450 m) as
/// the ring opens; and his vortex `ANIM_DEVA_VORTEX` rolling with the front's middle [data
/// cairo_city_a_boss mission.fev, loaded for Grindor: `sound_events`].
pub const IMPACT_SOUND: u32 = crc32(b"EXP_HUGE");
pub const POUND_SOUND: u32 = crc32(b"ANIM_DEVA_POUND");
pub const ROLL_SOUND: u32 = crc32(b"ANIM_DEVA_VORTEX");
/// The ones that are not in the level bank, loaded with his own.
pub const SOUNDS: [u32; 2] = [POUND_SOUND, ROLL_SOUND];
/// [port design] The impact's sounds reach the listener at this speed (m/s): the further off
/// the camera is, the later the boom.
pub const SOUND_SPEED: f32 = 340.0;
/// [port design] The impact's shake: the game's `MassiveCameraShake` (full to 20 m, felt to
/// 90 m, 2.5 s) [data global_cameras].
pub const IMPACT_SHAKE: u32 = crc32(b"MassiveCameraShake");

/// What the wave borrows from the install, read once when the character is loaded. [data]
#[derive(Clone, Debug)]
pub struct Dressing {
    /// Devastator's ring (his robot object's first `ShockWave`, cairo_city_a_boss): the base
    /// `def` scales.
    pub ring: Def,
    /// Grindor's own effects, by name from his pack: his ground slam's blast
    /// (`r_melee_endblast_blackout`) for the impact, his heavy landing's dust
    /// (`r_contactdust_biglanding`) and a building's sparks (`exp_bldg_electricity01`) along the
    /// front.
    pub impact: Option<Template>,
    pub dust: Option<Template>,
    pub sparks: Option<Template>,
}

/// The sound events `SOUNDS` are in: Devastator's mission bank.
pub fn sound_events(game_dir: &Path) -> Option<PathBuf> {
    characterlist::elsewhere("devastator").map(|e| game_dir.join(e.sounds))
}

/// Reads the wave's dressing: Devastator's ring from his pack, with the part of his library its
/// material draws with (the material and the textures it names), and the effects named in
/// `Dressing` from the character's own pack `pack`.
pub fn read(game_dir: &Path, pack: &str) -> Result<(Dressing, Library), String> {
    let boss = characterlist::elsewhere("devastator").ok_or("no Devastator entry")?;
    let his = Pack::open(&characterlist::pack_file(game_dir, boss.pack))?;
    let scene = crc32(boss.scene.as_bytes());
    let ring = his
        .of_type(pack::DATA)
        .filter_map(|chunk| DataFile::parse(his.data(chunk).to_vec()).ok())
        .find_map(|file| {
            let top = crate::character::find_label(file.root(), scene, 0, &mut HashSet::new())?;
            let object = top
                .fields()
                .into_iter()
                .flat_map(|(_, root)| root.get("things").into_iter().flat_map(|t| t.items()))
                .find(|t| t.type_name() == Some("Object"))?;
            crate::character::read_shockwaves(object).into_iter().next().map(|(_, def)| def)
        })
        .ok_or_else(|| format!("no ShockWave on {} in {}", boss.scene, boss.file))?;
    let mut all = Library::load(&his);
    let mut library = Library::default();
    if let Some(material) = ring.material.and_then(|id| all.materials.remove(&id).map(|m| (id, m))) {
        for param in &material.1.params {
            if let ParamValue::Texture(Some(t)) = param.value {
                if let Some(texture) = all.textures.remove(&t) {
                    library.textures.insert(t, texture);
                }
                if let Some(frames) = all.texture_frames.remove(&t) {
                    for f in &frames.frames {
                        if let Some(texture) = all.textures.remove(f) {
                            library.textures.insert(*f, texture);
                        }
                    }
                    library.texture_frames.insert(t, frames);
                }
            }
        }
        library.materials.insert(material.0, material.1);
    }
    let own = Pack::open(&characterlist::pack_file(game_dir, pack))?;
    let files: Vec<DataFile> = own.of_type(pack::DATA).filter_map(|chunk| DataFile::parse(own.data(chunk).to_vec()).ok()).collect();
    let named = |name: &str| {
        let label = crc32(name.as_bytes());
        files.iter().find_map(|f| find_deep(f.root(), label, &mut HashSet::new())).and_then(Template::read)
    };
    let dressing = Dressing {
        ring,
        impact: named("r_melee_endblast_blackout"),
        dust: named("r_contactdust_biglanding"),
        sparks: named("exp_bldg_electricity01"),
    };
    Ok((dressing, library))
}

/// The first node labelled `label` under `node`, at any depth (the templates sit inside clip
/// events and placed objects). Only typed objects are kept from a second visit: an array and
/// its first element can share a place.
fn find_deep<'a>(node: Node<'a>, label: u32, seen: &mut HashSet<(usize, Option<&'a str>)>) -> Option<Node<'a>> {
    if node.offset().is_some_and(|o| !seen.insert((o, node.type_name()))) {
        return None;
    }
    if node.label() == Some(label) {
        return Some(node);
    }
    node.fields().into_iter().map(|(_, n)| n).chain(node.items()).find_map(|n| find_deep(n, label, seen))
}

/// [port design] Half the arc's width: 70 degrees in all, chosen by the user (2026-10-24).
pub const HALF_ANGLE: f32 = 35.0 * std::f32::consts::PI / 180.0;
/// [port design] The ring's radius at the impact: the front is 4 m across there
/// (2 x 3.5 x sin 35 degrees).
pub const START_RADIUS: f32 = 3.5;
/// [port design] Full damage and push for the first third of the run, then down to a third at
/// its end (proposed with the plan, 2026-10-24).
pub const FALLOFF: Falloff = Falloff { full_until: 1.0 / 3.0, end_scale: 1.0 / 3.0 };

/// The wave's ring data: Devastator's ring (`base`, [data] cairo_city_a_boss) scaled to the
/// snaker's run. `life` is how long the snaker lives (min of its `timeTilDies` and its object's
/// TimeoutDeleter, as `weapons::Snaker::with_life`); the run is `speed x life`. [port design]
/// - the band's OUTER edge (what is seen first, and what reaches a target first: the query
///   takes anything inside the band) moves at the snaker's speed and ends `run` past where it
///   started, so the front ends where the original wave would have ended;
/// - the band's full width is his (70 m at the end of a 157 m run) scaled to the run, and the
///   ring's own radius runs slower by half the band's growth;
/// - fades keep their place along the ring's run (his are at 11 % and 43 % of it);
/// - the start band (his 5 m), height check (2 m), segment counts (a whole ring's: the arc
///   takes its share), material and knock-back (`specialCaseDamageStuff`) are his;
/// - the damage is the snaker's (120: the original wave's), with the loader's default damage
///   type (`BY_EXPLOSION | BY_KNOCKBACK`, where his data gives none) so it knocks back.
pub fn def(base: &Def, snaker: &SnakerTuning, life: f32) -> Def {
    let run = snaker.speed * life;
    let his_run = base.full_radius - base.start_radius;
    let k = if his_run > 0.0 { run / his_run } else { 1.0 };
    let width = base.ring_radius + (base.full_ring_radius - base.ring_radius) * k;
    // The outer edge: START + ring/2 at the start, full + width/2 at the end, `run` apart.
    let full = START_RADIUS + run + (base.ring_radius - width) * 0.5;
    let ring_run = full - START_RADIUS;
    let kr = if his_run > 0.0 { ring_run / his_run } else { 1.0 };
    let along = |r: f32| START_RADIUS + (r - base.start_radius) * kr;
    let loader = Def::default();
    Def {
        name: 0,
        start_radius: START_RADIUS,
        fade_in_radius: along(base.fade_in_radius),
        fade_out_radius: along(base.fade_out_radius),
        full_radius: full,
        full_ring_radius: width,
        speed: if life > 0.0 { ring_run / life } else { snaker.speed },
        damage: snaker.damage,
        damage_type: loader.damage_type,
        damage_type_small: loader.damage_type_small,
        damage_type_large: loader.damage_type_large,
        ..base.clone()
    }
}

/// The wave's object: its ring data with the falloff.
pub fn wave(base: &Def, snaker: &SnakerTuning, life: f32) -> ShockWave {
    let mut w = ShockWave::new(def(base, snaker, life));
    w.falloff = Some(FALLOFF);
    w
}

/// Opens the arc at `at` along the flat part of `way`; nothing when `way` is straight up or
/// down. The caller puts the ring on the ground (`start` finds it, `ShockWave::ground`).
pub fn fire(wave: &mut ShockWave, at: Vec3, way: Vec3, time: f32) -> bool {
    let Some(arc) = Arc::new(way, HALF_ANGLE) else { return false };
    wave.trigger_arc(at, time, arc);
    true
}

/// [port design] A surface whose normal is less than this up is a wall, not ground (the snaker's
/// own wall test takes 0.5, `weapons::Snaker::hits_wall` [game 0078e300]).
pub const WALL_NORMAL: f32 = 0.5;
/// [port design] How far along the missile's way, each side of its end, the surface it ended on
/// is looked for.
pub const SURFACE_PROBE: f32 = 1.0;
/// [port design] A wave off a wall opens this far out from it.
pub const WALL_OFF: f32 = 1.0;
/// [port design] How far below the impact the ground may be for a wave to open (a shot into a wall
/// higher than this, or one that ended in the air, opens none: the original's own impact stays).
pub const FALL: f32 = 8.0;

/// [port design] Where the wave opens for a missile that ended at `at` going `way`, and which way
/// it runs (flat): on the ground under the impact. A missile that ended on a wall (its surface's
/// normal under `WALL_NORMAL` up) opens it `WALL_OFF` out from the wall, at the wall's foot, its way
/// the shot's mirrored off the wall (a shot square into a wall runs back out from it). None when
/// there is no ground within `FALL` below, or the shot went straight down onto it.
pub fn start(at: Vec3, way: Vec3, world: &dyn Sight) -> Option<(Vec3, Vec3)> {
    let along = way.normalize_or_zero();
    let mut flat = way.truncate().normalize_or_zero();
    let mut base = at;
    if along != Vec3::ZERO
        && let Some(hit) = world.sight_mask(at - along * SURFACE_PROBE, at + along * SURFACE_PROBE, PUSH)
    {
        let n = hit.normal.normalize_or_zero();
        let out = n.truncate().normalize_or_zero();
        if n.z < WALL_NORMAL && out != Vec2::ZERO {
            let into = flat.dot(out);
            if into < 0.0 {
                flat -= 2.0 * into * out;
            }
            flat = flat.normalize_or(out);
            base = hit.point + out.extend(0.0) * WALL_OFF;
        }
    }
    if flat == Vec2::ZERO {
        return None;
    }
    let floor = world.sight_mask(base + Vec3::Z * PROBE_UP, base - Vec3::Z * FALL, PUSH)?;
    (floor.normal.normalize_or_zero().z > WALL_NORMAL).then(|| (floor.point, flat.extend(0.0)))
}

/// [port design] The ground under the wave: `LINES` + 1 lines out from the impact across the arc,
/// each sampled every `STEP` ahead of the front as it runs. A line climbs whatever is at most
/// `CLIMB` above its last sample (a crate, a kerb, a ramp) and goes down drops of up to `DROP`;
/// a wall taller than `CLIMB` (a level ray at that height meets it) or a deeper drop stops it, and
/// the band, the hits, the energy wall and the dust go no further along it. Not in the game: its
/// ring has one probe at its centre and is drawn flat (`shockwave.rs`).
pub const LINES: usize = 24;
pub const STEP: f32 = 1.5;
pub const CLIMB: f32 = 2.5;
pub const DROP: f32 = 6.0;
/// Rows of quads across the band at most (one every `STEP` up to this).
pub const MAX_ROWS: usize = 16;
/// [port design] A part of the front stopped by a wall stays there until the rest has run this
/// much further, then goes; once every line is stopped the wave is over as soon as its front has
/// run this far past the furthest stop (`end_radius`).
pub const FRONT_HOLD: f32 = 3.0;

#[derive(Clone, Debug)]
struct Line {
    way: Vec2,
    /// The ground's height every `STEP` from the impact (the first at the impact).
    heights: Vec<f32>,
    /// How far out it stops; infinite while open.
    stop: f32,
}

impl Line {
    fn height(&self, r: f32) -> f32 {
        let s = r.max(0.0) / STEP;
        let i = s as usize;
        match (self.heights.get(i), self.heights.get(i + 1)) {
            (Some(a), Some(b)) => a + (b - a) * s.fract(),
            _ => *self.heights.last().unwrap_or(&0.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ground {
    centre: Vec2,
    /// The arc's middle (as `Arc::angle` measures) and its whole span.
    middle: f32,
    span: f32,
    lines: Vec<Line>,
}

impl Ground {
    /// For the wave's first ring, on the ground already (`ShockWave::ground`).
    pub fn new(wave: &ShockWave) -> Option<Self> {
        let ring = wave.rings.first()?;
        let arc = ring.arc?;
        let span = arc.half_angle * 2.0;
        let floor = ring.centre.z - GROUND_LIFT;
        let lines = (0..=LINES)
            .map(|k| {
                let a = arc.angle() - arc.half_angle + span * k as f32 / LINES as f32;
                Line { way: Vec2::new(a.sin(), a.cos()), heights: vec![floor], stop: f32::INFINITY }
            })
            .collect();
        Some(Self { centre: ring.centre.truncate(), middle: arc.angle(), span, lines })
    }

    /// Samples every open line out to `STEP` past where the band's outer edge will be after the
    /// next `dt` of the ring's run.
    pub fn advance(&mut self, wave: &ShockWave, world: &dyn Sight, dt: f32) {
        let Some(ring) = wave.rings.first() else { return };
        let want = wave.band(ring.radius + wave.def.speed * dt).1 + STEP;
        let centre = self.centre;
        for line in &mut self.lines {
            while line.stop.is_infinite() && (line.heights.len() - 1) as f32 * STEP < want {
                let d = line.heights.len() as f32 * STEP;
                let top = line.heights.last().copied().unwrap_or(0.0) + CLIMB;
                let flat = |d: f32| (centre + line.way * d).extend(top);
                if let Some(wall) = world.sight_mask(flat(d - STEP), flat(d), PUSH) {
                    line.stop = (wall.point.truncate() - centre).length().max(d - STEP);
                    break;
                }
                match world.sight_mask(flat(d), flat(d) - Vec3::Z * (CLIMB + DROP), PUSH) {
                    Some(hit) if hit.normal.normalize_or_zero().z > WALL_NORMAL => line.heights.push(hit.point.z),
                    _ => {
                        line.stop = d - STEP;
                        break;
                    }
                }
            }
        }
    }

    /// The two lines either side of `angle` and how far it is from the first to the second.
    fn around(&self, angle: f32) -> (&Line, &Line, f32) {
        let off = (angle - self.middle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let u = ((off / self.span + 0.5) * LINES as f32).clamp(0.0, LINES as f32);
        let i = (u as usize).min(LINES - 1);
        (&self.lines[i], &self.lines[i + 1], u - i as f32)
    }

    /// How far out the wave goes at `angle`: the nearer stop of the lines either side.
    pub fn reach(&self, angle: f32) -> f32 {
        let (a, b, _) = self.around(angle);
        a.stop.min(b.stop)
    }

    /// The ground's height `r` out at `angle`.
    pub fn height(&self, angle: f32, r: f32) -> f32 {
        let (a, b, k) = self.around(angle);
        a.height(r) + (b.height(r) - a.height(r)) * k
    }

    /// The ground `r` out at `angle`, or at its stop when `r` is past it.
    pub fn point(&self, angle: f32, r: f32) -> Vec3 {
        let r = r.min(self.reach(angle));
        (self.centre + Vec2::new(angle.sin(), angle.cos()) * r).extend(self.height(angle, r))
    }

    /// Whether some line still runs.
    pub fn open(&self) -> bool {
        self.lines.iter().any(|l| l.stop.is_infinite())
    }

    /// The ring radius at which the wave is over: its own end, or sooner once every line is
    /// stopped (`FRONT_HOLD` past the furthest stop).
    pub fn end_radius(&self, wave: &ShockWave) -> f32 {
        let d = &wave.def;
        if self.open() {
            return d.full_radius;
        }
        let furthest = self.lines.iter().map(|l| l.stop).fold(0.0, f32::max) + FRONT_HOLD;
        // The band's outer edge is r + half its width, and the width grows straight with r.
        let g = (d.full_ring_radius - d.ring_radius) / (d.full_radius - d.start_radius);
        let r = (furthest - 0.5 * (d.ring_radius - g * d.start_radius)) / (1.0 + 0.5 * g);
        r.min(d.full_radius)
    }

    /// The targets the wave may reach this update: the ones in front of their angle's stop, on the
    /// ground there (within the ring's `height_check` of it, the ring's own test against its
    /// centre's height), each given the ring's height so its own test passes.
    pub fn candidates(&self, wave: &ShockWave, all: &[Candidate]) -> Vec<Candidate> {
        let Some(ring) = wave.rings.first() else { return Vec::new() };
        all.iter()
            .filter_map(|c| {
                let off = c.position.truncate() - self.centre;
                let angle = off.x.atan2(off.y);
                let near = (off.length() - c.radius).max(0.0);
                if near > self.reach(angle) {
                    return None;
                }
                let floor = self.height(angle, near) + GROUND_LIFT;
                if wave.def.height_check >= 0.0 && (c.position.z - floor).abs() >= wave.def.height_check {
                    return None;
                }
                Some(Candidate { position: c.position.truncate().extend(ring.centre.z), ..*c })
            })
            .collect()
    }
}

/// [port design] The band of the wave's `ring` laid on its ground: as `ShockWave::mesh` (the same
/// alpha, u across the arc 0..1, v 0 inside and 1 outside, normals leaning in and out), but a grid
/// of quads, one column between each two lines and up to `MAX_ROWS` rows across the band, each
/// point at the ground's height there (plus `GROUND_LIFT`) and pulled in to its line's stop.
pub fn mesh(wave: &ShockWave, ring: &Ring, ground: &Ground, vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>) {
    let alpha = wave.alpha(ring);
    if alpha <= 0.0 || wave.def.material.is_none() {
        return;
    }
    let (inner, outer) = wave.band(ring.radius);
    let inner = inner.max(0.0);
    let rows = (((outer - inner) / STEP).ceil() as usize).clamp(1, MAX_ROWS);
    let base = vertices.len() as u32;
    for (k, line) in ground.lines.iter().enumerate() {
        for j in 0..=rows {
            let v = j as f32 / rows as f32;
            let r = (inner + (outer - inner) * v).min(line.stop);
            let lean = v * 2.0 - 1.0;
            vertices.push(Vertex {
                position: (ground.centre + line.way * r).extend(line.height(r) + GROUND_LIFT),
                normal: Vec3::new(line.way.x * lean, line.way.y * lean, 1.0).normalize(),
                uv: Vec2::new(k as f32 / LINES as f32, v),
                alpha,
            });
        }
    }
    let across = rows as u32 + 1;
    for k in 0..LINES as u32 {
        for j in 0..rows as u32 {
            let a = base + k * across + j;
            let (b, c) = (a + 1, a + across);
            indices.extend([a, c, b, b, c, c + 1]);
        }
    }
}

/// [port design] The impact, dressed in the game's own pieces (all proposed with the plan,
/// 2026-10-24; tuned on captures): his ground slam's blast this many times its size, his heavy
/// landing's dust this many times, and the wave object's Light this much wider and brighter for
/// a short flash.
/// (Cut from 4 and 2 on the user's word after playing it, 2026-10-24: too much explosion.)
pub const IMPACT_BLAST_SCALE: f32 = 2.0;
pub const IMPACT_DUST_SCALE: f32 = 1.3;
pub const FLASH_RANGE: f32 = 4.0;
pub const FLASH_COLOUR: f32 = 2.5;
/// The flash's deleter: its time and fade start (the wave Light's own are 1.75 and 1.45).
pub const FLASH_TIME: f32 = 0.6;
pub const FLASH_FADE_START: f32 = 0.1;

/// [port design] Along the front: a row of dust puffs every `DUST_EVERY` seconds (the first half
/// that in), from `DUST_ROW.0` across the arc at the impact to `DUST_ROW.1` at the end of the
/// run, never more than `DUST_LIVE` alive (a puff lives `DUST_LIFE`: the landing dust's longest
/// element ends at 1.7 s [data]), growing from the first scale to the second as the ring runs;
/// a burst of sparks every `SPARKS_EVERY`. The cost sets the counts: every element of every
/// effect is a mesh rebuilt each frame (`effects::run`), and a puff has 9; the plan's 12 puffs
/// with 9 wall copies cost 1.3 ms a frame more on average and up to 8 ms more on the worst
/// frame (measured 2026-10-24, `notes\grindor-cinematic-wave.md`). Cut again after the user played
/// it (2026-10-24: "reduced significantly"): from a puff every 0.3 s (6 alive, up to 2 a row, 1.2 to
/// 2 times) and sparks every 0.6 s, to a puff every 0.6 s (3 alive, 1 to 1.5 times) and sparks every
/// 1.2 s (two bursts a wave, was three).
pub const DUST_EVERY: f32 = 0.6;
pub const DUST_ROW: (u32, u32) = (1, 1);
pub const DUST_LIVE: usize = 3;
pub const DUST_LIFE: f32 = 1.7;
pub const DUST_SCALE: (f32, f32) = (1.0, 1.5);
pub const SPARKS_EVERY: f32 = 1.2;
/// How far behind the band's outer edge the puffs rise: on the bright band, not ahead of it.
pub const KICK_BACK: f32 = 2.0;

/// [port design] The front's energy wall: the wave object's own emitter (`w_exp_energoncannon`:
/// spark streamers and a glow along a 20 m line across its way, x -10..10 [data]) laid
/// `FRONT_SLOTS` times across the arc, each riding the band's outer edge with its line along the
/// arc, scaled to its share of the arc with some overlap, between `FRONT_SCALE_MIN` and
/// `FRONT_SCALE_MAX`. Devastator's band alone is faint smoke (`notes\devastator.md` "Captures"):
/// this is what makes the front read. Each copy's particles are `FRONT_SIZE` times their own
/// size (`Template::scaled`; the frame's scale only spreads them along the line).
pub const FRONT_SLOTS: u32 = 5;
pub const FRONT_SIZE: f32 = 1.5;
pub const FRONT_LINE: f32 = 20.0;
pub const FRONT_OVERLAP: f32 = 1.25;
pub const FRONT_SCALE_MIN: f32 = 0.35;
pub const FRONT_SCALE_MAX: f32 = 1.5;

/// The frame copy `slot` of the front's emitter rides, while the wave's first ring lives: x along
/// the arc, y out from the impact, z up, at the band's outer edge on the ground there, scaled
/// (uniformly) to its share of the arc. A copy whose way is stopped waits at the stop, and goes
/// once the front has run `FRONT_HOLD` past it.
pub fn front_frame(wave: &ShockWave, ground: &Ground, slot: u32) -> Option<glam::Affine3A> {
    let ring = wave.rings.first()?;
    let arc = ring.arc?;
    let spread = arc.half_angle * 0.85;
    let u = (slot as f32 + 0.5) / FRONT_SLOTS as f32 * 2.0 - 1.0;
    let angle = arc.angle() + u * spread;
    let outer = wave.band(ring.radius).1;
    if outer > ground.reach(angle) + FRONT_HOLD {
        return None;
    }
    let share = outer * 2.0 * spread / FRONT_SLOTS as f32;
    let scale = (share * FRONT_OVERLAP / FRONT_LINE).clamp(FRONT_SCALE_MIN, FRONT_SCALE_MAX);
    let y = Vec3::new(angle.sin(), angle.cos(), 0.0);
    let turn = glam::Mat3::from_cols(y.cross(Vec3::Z), y, Vec3::Z);
    Some(glam::Affine3A::from_mat3_translation(turn * scale, ground.point(angle, outer) + Vec3::Z * GROUND_LIFT))
}

/// What the front kicks up this update, on the ground (`Ground::point`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kick {
    Dust { at: Vec3, scale: f32 },
    /// Sparks, with the way out from the impact (the effect sprays along its +x).
    Sparks { at: Vec3, way: Vec3 },
}

/// [port design] One wave's front dressing: when to kick up dust and sparks, and where.
#[derive(Clone, Debug, Default)]
pub struct Front {
    age: f32,
    next_dust: f32,
    next_sparks: f32,
    /// How many points have been laid along the arc (the next one's place in the sequence).
    laid: u32,
    /// The ages the live puffs were made at.
    puffs: Vec<f32>,
}

impl Front {
    /// The kicks of the update that took the wave's age by `dt`, along its first ring (none once
    /// it is over); none where its way is stopped short of the front.
    pub fn step(&mut self, wave: &ShockWave, ground: &Ground, dt: f32) -> Vec<Kick> {
        self.age += dt;
        let age = self.age;
        self.puffs.retain(|&made| age - made < DUST_LIFE);
        let mut out = Vec::new();
        let (Some(ring), d) = (wave.rings.first(), &wave.def) else { return out };
        let Some(arc) = ring.arc else { return out };
        let run = ((ring.radius - d.start_radius) / (d.full_radius - d.start_radius)).clamp(0.0, 1.0);
        let out_to = (wave.band(ring.radius).1 - KICK_BACK).max(d.start_radius);
        let point = |laid: &mut u32| {
            // An even spread across the arc that never repeats (golden-ratio steps), kept off its
            // very edges, where the band thins out.
            let u = (0.5 + *laid as f32 * 0.618_034).fract() * 2.0 - 1.0;
            *laid += 1;
            let angle = arc.angle() + u * arc.half_angle * 0.9;
            let way = Vec3::new(angle.sin(), angle.cos(), 0.0);
            (out_to <= ground.reach(angle)).then(|| (ground.point(angle, out_to), way))
        };
        if self.next_dust == 0.0 {
            self.next_dust = DUST_EVERY * 0.5;
        }
        while age >= self.next_dust {
            self.next_dust += DUST_EVERY;
            let row = DUST_ROW.0 + ((DUST_ROW.1 - DUST_ROW.0) as f32 * run).round() as u32;
            for _ in 0..row {
                if self.puffs.len() >= DUST_LIVE {
                    break;
                }
                let Some((at, _)) = point(&mut self.laid) else { continue };
                self.puffs.push(age);
                out.push(Kick::Dust { at, scale: DUST_SCALE.0 + (DUST_SCALE.1 - DUST_SCALE.0) * run });
            }
        }
        while age >= self.next_sparks {
            self.next_sparks += SPARKS_EVERY;
            if let Some((at, way)) = point(&mut self.laid) {
                out.push(Kick::Sparks { at, way });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shockwave::{Candidate, Hit};

    /// Devastator's ring, as `shockwave.rs` tests it (cairo_city_a_boss mission.str#1) [data],
    /// with his knock-back and stun (`notes\devastator.md`).
    fn devastator() -> Def {
        Def {
            material: Some(1),
            height_check: 2.0,
            start_radius: 3.0,
            fade_in_radius: 20.0,
            fade_out_radius: 70.0,
            full_radius: 160.0,
            ring_radius: 5.0,
            full_ring_radius: 70.0,
            speed: 75.0,
            start_segments: 25,
            full_segments: 100,
            damage_type: 0,
            damage_type_small: 0,
            damage_type_large: 0,
            special: crate::shockwave::SpecialCase { stun_time: 1.0, knock_back_speed: 15.0, knock_back_angle: 35f32.to_radians(), ..Default::default() },
            damage: 25.0,
            ..Def::default()
        }
    }

    /// Grindor's `snakerInfo` (`Blackout_MissileAttribute`, global_states.txt 80472) [data].
    fn grindor() -> SnakerTuning {
        SnakerTuning { speed: 60.0, damage: 120.0, time_til_dies: 1.75, damage_flags: 0 }
    }

    fn at(id: u32, x: f32, y: f32) -> Candidate {
        Candidate { id, position: Vec3::new(x, y, 0.0), radius: 0.5, size: None }
    }

    /// Runs a wave fired from the origin along +y for its whole life; who it hit, how hard and when.
    fn run(candidates: &[Candidate]) -> Vec<(Hit, f32)> {
        let mut w = wave(&devastator(), &grindor(), 1.75);
        assert!(fire(&mut w, Vec3::ZERO, Vec3::new(0.0, 1.0, -0.3), 0.0));
        let mut hits = Vec::new();
        let mut t = 0.0;
        while w.active() {
            t += 0.032;
            w.update(0.032, 0, None, candidates, |h| {
                hits.push((*h, t));
                true
            });
        }
        hits
    }

    #[test]
    fn the_ring_is_devastators_scaled_to_the_snakers_run() {
        let d = def(&devastator(), &grindor(), 1.75);
        let outer = |d: &Def, r: f32| ShockWave::new(d.clone()).band(r).1;
        // The front's outer edge runs 60 m/s for 1.75 s: 105 m, where the original wave ended.
        assert!((outer(&d, d.start_radius) - 6.0).abs() < 1e-4, "3.5 + half the 5 m start band");
        assert!((outer(&d, d.full_radius) - 111.0).abs() < 1e-3);
        assert!((d.speed * 1.75 - (d.full_radius - d.start_radius)).abs() < 1e-3, "the ring ends with the snaker's life");
        let k = 105.0 / 157.0;
        assert!((d.full_ring_radius - (5.0 + 65.0 * k)).abs() < 1e-4, "his band, scaled to the run");
        let kr = (d.full_radius - 3.5) / 157.0;
        assert!((d.fade_in_radius - (3.5 + 17.0 * kr)).abs() < 1e-4);
        assert!((d.fade_out_radius - (3.5 + 67.0 * kr)).abs() < 1e-4);
        assert_eq!((d.damage, d.height_check), (120.0, 2.0));
        assert_eq!((d.ring_radius, d.start_segments, d.full_segments, d.material), (5.0, 25, 100, Some(1)));
        assert_eq!(d.damage_type, 0x40800, "knocks back where his data gives no type");
        assert_eq!(d.special.knock_back_speed, 15.0);
        // A shorter object life shortens the run: 60 m in 1 s.
        let short = def(&devastator(), &grindor(), 1.0);
        assert!((outer(&short, short.full_radius) - 66.0).abs() < 1e-3);
    }

    #[test]
    fn the_front_is_4_m_across_at_the_start_and_about_125_at_the_end() {
        let d = def(&devastator(), &grindor(), 1.75);
        let across = |r: f32| 2.0 * r * HALF_ANGLE.sin();
        assert!((across(d.start_radius) - 4.0).abs() < 0.02);
        let end = ShockWave::new(d.clone()).band(d.full_radius).1;
        assert!((across(end) - 127.3).abs() < 0.5, "{}", across(end));
    }

    #[test]
    fn it_hits_inside_the_arc_and_nothing_behind_or_beside_it() {
        let ahead = at(1, 0.0, 30.0);
        let edge = at(2, 30.0 * 34f32.to_radians().sin(), 30.0 * 34f32.to_radians().cos());
        let beside = at(3, 30.0 * 40f32.to_radians().sin(), 30.0 * 40f32.to_radians().cos());
        let behind = at(4, 0.0, -30.0);
        let side = at(5, 30.0, 0.0);
        let mut ids: Vec<u32> = run(&[ahead, edge, beside, behind, side]).iter().map(|(h, _)| h.target).collect();
        ids.sort();
        assert_eq!(ids, [1, 2]);
    }

    #[test]
    fn each_target_once_full_strength_near_and_a_third_at_the_end() {
        let near = at(1, 0.0, 20.0);
        let far = at(2, 0.0, 104.0);
        let hits = run(&[near, far]);
        assert_eq!(hits.len(), 2, "{hits:?}");
        let (n, f) = (&hits[0].0, &hits[1].0);
        assert_eq!((n.target, f.target), (1, 2));
        assert_eq!(n.packet.amount, 120.0, "the first third is full");
        assert_eq!(n.packet.knockback_speed, 15.0);
        // At 104 m the ring has run 93 % of the way: close to a third (48.6).
        assert!(f.packet.amount > 45.0 && f.packet.amount < 52.0, "{}", f.packet.amount);
        assert!((f.packet.knockback_speed / 15.0 - f.packet.amount / 120.0).abs() < 1e-5, "the push weakens with the damage");
        assert!((f.force.length() / n.force.length() - f.packet.amount / 120.0).abs() < 1e-4);
        // Pushed away from the impact and up at 45 degrees.
        assert!(n.packet.direction.y > 0.7 && n.packet.direction.z > 0.7);
        // Reached when the front's outer edge (6 m out at the start, 60 m/s) meets its near
        // side (103.5 m).
        assert!((hits[1].1 - (103.5 - 6.0) / 60.0).abs() < 0.04, "{}", hits[1].1);
        assert!((hits[0].1 - (19.5 - 6.0) / 60.0).abs() < 0.04, "{}", hits[0].1);
    }

    #[test]
    fn it_is_over_once_the_snaker_would_be() {
        let mut w = wave(&devastator(), &grindor(), 1.75);
        fire(&mut w, Vec3::ZERO, Vec3::Y, 0.0);
        let mut t = 0.0f32;
        while w.active() {
            w.update(0.032, 0, None, &[], |_| true);
            t += 0.032;
        }
        assert!((t - 1.75).abs() < 0.04, "{t}");
    }

    #[test]
    fn a_shot_straight_down_opens_nothing() {
        let mut w = wave(&devastator(), &grindor(), 1.75);
        assert!(!fire(&mut w, Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), 0.0));
        assert!(!w.active());
    }

    fn arena(boxes: &[(Vec3, Vec3)]) -> crate::sim::Arena {
        crate::sim::Arena { boxes: boxes.iter().map(|&(min, max)| crate::sim::Box3 { min, max }).collect(), ..Default::default() }
    }

    /// A wave opened on the arena's floor at the origin along +y, its ground sampled.
    fn opened(world: &crate::sim::Arena) -> (ShockWave, Ground) {
        let mut w = wave(&devastator(), &grindor(), 1.75);
        let (at, way) = start(Vec3::new(0.0, 0.0, 0.3), Vec3::new(0.0, 1.0, -0.3), world).expect("ground");
        assert!(fire(&mut w, at, way, 0.0));
        w.ground(0, at.z);
        let mut g = Ground::new(&w).unwrap();
        g.advance(&w, world, 0.0);
        (w, g)
    }

    /// One update of the wave as the game runs it: the ground sampled ahead, then the ring with the
    /// targets its ground lets through.
    fn tick(w: &mut ShockWave, g: &mut Ground, world: &crate::sim::Arena, candidates: &[Candidate], hits: &mut Vec<Hit>) {
        g.advance(w, world, 0.032);
        let let_through = g.candidates(w, candidates);
        w.update(0.032, 0, None, &let_through, |h| {
            hits.push(*h);
            true
        });
        if w.rings.first().is_some_and(|r| r.radius >= g.end_radius(w)) {
            w.rings.clear();
        }
    }

    /// A wall 6 m tall 30 m out: nothing behind it is hit, the band, the energy wall and the dust
    /// stay in front of it, and the wave is over soon after its front reaches it.
    #[test]
    fn a_wall_stops_it() {
        let world = arena(&[(Vec3::new(-200.0, 30.0, 0.0), Vec3::new(200.0, 31.0, 6.0))]);
        let (mut w, mut g) = opened(&world);
        let near = at(1, 0.0, 20.0);
        let behind = at(2, 0.0, 45.0);
        let (mut hits, mut front, mut t) = (Vec::new(), Front::default(), 0.0f32);
        while w.active() {
            tick(&mut w, &mut g, &world, &[near, behind], &mut hits);
            t += 0.032;
            let Some(ring) = w.rings.first().copied() else { break };
            let (mut v, mut i) = (Vec::new(), Vec::new());
            mesh(&w, &ring, &g, &mut v, &mut i);
            assert!(v.iter().all(|p| p.position.y <= 30.0 + 1e-3), "the band stays in front of the wall");
            for s in 0..FRONT_SLOTS {
                if let Some(f) = front_frame(&w, &g, s) {
                    assert!(f.translation.y <= 30.0 + 1e-3);
                }
            }
            for kick in front.step(&w, &g, 0.032) {
                let (Kick::Dust { at, .. } | Kick::Sparks { at, .. }) = kick;
                assert!(at.y <= 30.0 + 1e-3);
            }
        }
        assert_eq!(hits.iter().map(|h| h.target).collect::<Vec<_>>(), [1]);
        assert!(!g.open());
        // The front (6 m out at the start, 60 m/s) reaches the wall's corners, 30 / cos 35 = 36.6 m
        // out, in about half a second; the wave goes once it has run FRONT_HOLD past them.
        assert!(t < 0.75, "{t}");
    }

    /// A platform 2 m tall from 20 to 30 m out: the wave climbs it, hurts who stands on it, runs
    /// down its far side and on; a 6 m wall would have stopped it.
    #[test]
    fn it_climbs_what_is_low_enough() {
        let world = arena(&[(Vec3::new(-200.0, 20.0, 0.0), Vec3::new(200.0, 30.0, 2.0))]);
        let (mut w, mut g) = opened(&world);
        let on_top = Candidate { position: Vec3::new(0.0, 25.0, 2.0), ..at(1, 0.0, 0.0) };
        let beyond = at(2, 0.0, 45.0);
        let (mut hits, mut tall_seen) = (Vec::new(), false);
        while w.active() {
            tick(&mut w, &mut g, &world, &[on_top, beyond], &mut hits);
            let Some(ring) = w.rings.first().copied() else { break };
            let (mut v, mut i) = (Vec::new(), Vec::new());
            mesh(&w, &ring, &g, &mut v, &mut i);
            for p in v.iter().map(|v| v.position).filter(|p| p.y > 21.6 && p.y < 28.4) {
                assert!((p.z - (2.0 + GROUND_LIFT)).abs() < 1e-3, "on the platform: {p}");
                tall_seen = true;
            }
            for p in v.iter().map(|v| v.position).filter(|p| p.y > 31.6) {
                assert!((p.z - GROUND_LIFT).abs() < 1e-3, "back on the floor: {p}");
            }
        }
        assert!(tall_seen);
        let mut ids: Vec<u32> = hits.iter().map(|h| h.target).collect();
        ids.sort();
        assert_eq!(ids, [1, 2]);
        assert!(g.open());
    }

    /// A shot into a wall opens the wave at the wall's foot, running back out mirrored off it; a
    /// shot high up a wall, or into the air, opens none; one into the floor opens where it lands.
    #[test]
    fn a_shot_into_a_wall_opens_at_its_foot() {
        let world = arena(&[(Vec3::new(-200.0, 30.0, 0.0), Vec3::new(200.0, 31.0, 20.0))]);
        let (at, way) = start(Vec3::new(0.0, 30.0, 3.0), Vec3::new(0.0, 1.0, -0.1), &world).unwrap();
        assert!((at - Vec3::new(0.0, 30.0 - WALL_OFF, 0.0)).length() < 1e-4, "{at}");
        assert!((way - Vec3::new(0.0, -1.0, 0.0)).length() < 1e-5, "{way}");
        let (_, slant) = start(Vec3::new(5.0, 30.0, 3.0), Vec3::new(0.6, 0.8, 0.0), &world).unwrap();
        assert!((slant - Vec3::new(0.6, -0.8, 0.0)).length() < 1e-5, "{slant}");
        assert!(start(Vec3::new(0.0, 30.0, 12.0), Vec3::Y, &world).is_none(), "too high up the wall");
        assert!(start(Vec3::new(0.0, 0.0, 20.0), Vec3::Y, &world).is_none(), "in the air");
        let (floor, ahead) = start(Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 1.0, -0.5), &world).unwrap();
        assert!((floor - Vec3::new(0.0, 10.0, 0.0)).length() < 1e-4 && (ahead - Vec3::Y).length() < 1e-5);
        // Opened at the wall's foot, it runs away from the wall and is not stopped by it.
        let mut w = wave(&devastator(), &grindor(), 1.75);
        fire(&mut w, at, way, 0.0);
        w.ground(0, at.z);
        let mut g = Ground::new(&w).unwrap();
        g.advance(&w, &world, 0.0);
        assert!(g.open() && g.reach(-std::f32::consts::PI).is_infinite());
    }

    /// The front's kicks over a whole wave: dust rows on the band inside the arc, never more than
    /// the cap alive, growing as it runs; sparks now and then; nothing once it is over.
    #[test]
    fn the_front_kicks_up_dust_and_sparks_inside_the_arc_within_the_cap() {
        let world = arena(&[]);
        let (mut w, mut g) = opened(&world);
        let mut front = Front::default();
        let (mut dust, mut sparks, mut scales) = (Vec::new(), 0, Vec::new());
        let mut live: Vec<f32> = Vec::new();
        let mut t = 0.0f32;
        while w.active() {
            tick(&mut w, &mut g, &world, &[], &mut Vec::new());
            t += 0.032;
            let ring = w.rings.first().copied();
            for kick in front.step(&w, &g, 0.032) {
                let ring = ring.expect("kicks only while the ring lives");
                let (inner, outer) = w.band(ring.radius);
                match kick {
                    Kick::Dust { at, scale } => {
                        let r = at.truncate().length();
                        assert!(r <= outer && r >= inner.min(outer - KICK_BACK) - 1e-3, "{r} in {inner}..{outer}");
                        assert!(at.x.atan2(at.y).abs() <= HALF_ANGLE, "inside the arc");
                        dust.push(at);
                        scales.push(scale);
                        live.retain(|&made| t - made < DUST_LIFE);
                        live.push(t);
                        assert!(live.len() <= DUST_LIVE);
                    }
                    Kick::Sparks { at, way } => {
                        sparks += 1;
                        assert!((way.truncate().normalize() - at.truncate().normalize()).length() < 1e-3, "sprays outward");
                    }
                }
            }
        }
        assert!((DUST_LIVE - 1..=DUST_LIVE).contains(&dust.len()), "{}", dust.len());
        assert!((2..=3).contains(&sparks), "{sparks}");
        assert!(scales.first().unwrap() < scales.last().unwrap() && *scales.last().unwrap() <= DUST_SCALE.1);
        // Spread across the arc: some left of the middle, some right.
        assert!(dust.iter().any(|p| p.x < -5.0) && dust.iter().any(|p| p.x > 5.0));
        assert!(front.step(&w, &g, 0.032).is_empty(), "nothing after the wave");
    }

    /// The energy wall's copies stand on the band's outer edge across the arc, their lines along
    /// it, growing with the front until they reach their own size.
    #[test]
    fn the_front_wall_rides_the_outer_edge_across_the_arc() {
        let world = arena(&[]);
        let (mut w, mut g) = opened(&world);
        tick(&mut w, &mut g, &world, &[], &mut Vec::new());
        let early: Vec<_> = (0..FRONT_SLOTS).map(|s| front_frame(&w, &g, s).unwrap()).collect();
        for _ in 0..40 {
            tick(&mut w, &mut g, &world, &[], &mut Vec::new());
        }
        let ring = w.rings[0];
        let outer = w.band(ring.radius).1;
        let late: Vec<_> = (0..FRONT_SLOTS).map(|s| front_frame(&w, &g, s).unwrap()).collect();
        for f in &late {
            let at = Vec3::from(f.translation);
            assert!((at.truncate().length() - outer).abs() < 1e-3);
            assert!(at.x.atan2(at.y).abs() < HALF_ANGLE);
            // Its line (x) runs along the arc: square to the way out.
            assert!(Vec3::from(f.matrix3.x_axis).dot(at.normalize()).abs() < 1e-4);
        }
        let last = late.len() - 1;
        assert!(Vec3::from(late[0].translation).x < 0.0 && Vec3::from(late[last].translation).x > 0.0, "left to right");
        let size = |f: &glam::Affine3A| Vec3::from(f.matrix3.x_axis).length();
        assert!((size(&early[0]) - FRONT_SCALE_MIN).abs() < 1e-5 && size(&late[0]) > size(&early[0]) && size(&late[0]) <= FRONT_SCALE_MAX);
        while w.active() {
            tick(&mut w, &mut g, &world, &[], &mut Vec::new());
        }
        assert!(front_frame(&w, &g, 0).is_none(), "gone with the ring");
    }

    #[test]
    fn the_drawn_arc_spans_70_degrees_round_the_shot() {
        let mut w = wave(&devastator(), &grindor(), 1.75);
        // Fired along +x: the arc runs from 55 to 125 degrees (sin, cos measured from +y).
        fire(&mut w, Vec3::ZERO, Vec3::X, 0.0);
        w.update(0.5, 0, None, &[], |_| true);
        let ring = w.rings[0];
        let (mut v, mut i) = (Vec::new(), Vec::new());
        w.mesh(&ring, &mut v, &mut i);
        let n = w.segments(&ring) as usize;
        assert!(n >= 5, "{n}");
        assert_eq!((v.len(), i.len()), (4 * n, 6 * n));
        let angle = |p: Vec3| p.x.atan2(p.y).to_degrees();
        assert!((angle(v[0].position) - 55.0).abs() < 0.01, "{}", angle(v[0].position));
        assert!((angle(v[v.len() - 1].position) - 125.0).abs() < 0.01);
        assert!(v.iter().all(|p| p.position.x > 0.0), "all of it ahead of the impact");
        assert!((v[v.len() - 1].uv.x - 1.0).abs() < 1e-4);
    }
}
