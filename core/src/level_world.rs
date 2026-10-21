//! One of the game's own levels as the world the movement code stands in: `sim::World`
//! answered from `collision_world::StaticWorld` and the mask presets.
//!
//! What is the game's and what is not:
//! - [data] the shapes, their placement and their masks (`collision_world`, `collision_masks`).
//! - [game] a query takes a body when the body's mask word shares a bit with the query's
//!   mask (the sweep 005b2630, `notes\collision-world-query.md`). [assumed] that word is the
//!   SEND mask: every static preset takes nothing, and the gun traces (mask 2, PUNCH) must
//!   stop on walls. Settled by finding what fills the record's +0x18.
//! - [assumed] the ground under the player's character and under a wheel is asked for with
//!   `CHARACTER_MASK` (see it). No ray of the original was found to be "the ground probe of
//!   a walking character" (`notes\character-world-probes.md`): its bodies meet the world
//!   through the pair tests.
//! - [stand-in] `floor` and `push_out` themselves: the original moves a character by its
//!   swept pair tests against the world (not ported); here the floor is one ray down and
//!   the walls are a ring of short rays.
//! - [assumed] primitives flagged `released` (under a DamageLink / DetachLink) are not there.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use glam::{Vec2, Vec3};

use crate::climb::{self, Ledge};
use crate::collision_masks::{self as bits, Masks, Presets};
use crate::collision_world::{Owner, StaticWorld, WorldHit};
use crate::melee;
use crate::navmesh::{NavMesh, FLAG_CAN_NOT_CLIMB};
use crate::sim::{World, STEP};
use crate::vehicle::GroundHit;

/// What the PLAYER's character meets: PUSH | AVATAR_PUSH | PLAYER_PUSH (0x45).
/// [game] 007c84c0 and 007c86a0 test an avatar's step (P to P + V * dt) with this mask
/// (`notes\character-world-probes.md`), and the avatar's body is created with it for the
/// player, with 0x0d for an AI (006e7b60, `notes\character-world-contact.md`). The first
/// version here used the ledge catch's 0x5d, which has AI_PUSH too: he then stood on
/// enemy-only blockers (a box at z 23 and a barrier at z 65 over Shanghai's (0, 0)).
pub const CHARACTER_MASK: u16 = bits::PUSH | bits::AVATAR_PUSH | bits::PLAYER_PUSH;

/// [game] the mask of the ledge-catch sweep: PUSH | AVATAR_PUSH | AI_PUSH | GROUND | PLAYER_PUSH.
pub const LEDGE_MASK: u16 = 0x5d;

/// Persistent inputs to the climbable-mesh query. `selector` is the
/// `controller_is_ai` result passed as 006f7130 param_5: AI queries skip the
/// ceiling/layer equality gate; player queries require matching open layers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClimbQueryContext {
    /// Cached cell proxy for owner+0x398. [stand-in] The original's 006fb130 uses
    /// this cached cell plus eight global-LCG vertex samples to choose its walk
    /// seed; this port starts the classifier at the cached triangle directly.
    pub triangle: Option<usize>,
    pub selector: bool,
}

/// A ledge query returns the updated seed even when no ledge was found, matching
/// the original caller's unconditional write back to owner+0x398.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClimbQueryResult {
    pub ledge: Option<Ledge>,
    pub context: ClimbQueryContext,
}

/// How far under the feet the floor probe looks, and its ball's radius. [stand-in]
/// Added to the pencil's swept box when choosing pair nodes, metres. [stand-in] a margin for the box
/// broad phase that replaces 006e5e00's; it only has to keep every primitive the sweep can reach.
const RAY_TOL_PAIR: f32 = 0.05;
/// The avatar body's linear damping per step (+0x2c, 006e96c0). [trace] 0.9995 in every captured
/// Bumblebee body (`notes\driving.md`); [assumed] the robot's pencil body has it too.
const AVATAR_DAMPING: f32 = 0.9995;
/// [game] 00853420: the ground probe's mask for the player's character (0x09 for an AI's).
const PLAYER_PROBE_MASK: u16 = 0x41;
const AI_PROBE_MASK: u16 = 0x09;
/// An AI character's holder masks in both words. [assumed] PUSH | AVATAR_PUSH | AI_PUSH: the player's
/// `CHARACTER_MASK` with AI_PUSH for PLAYER_PUSH, as the two ground probes differ (0x41 / 0x09), so an AI walks
/// into the level's AI-only walls (`StaticAIPushOnlyObj`) and the player does not.
pub const AI_CHARACTER_MASK: u16 = bits::PUSH | bits::AVATAR_PUSH | bits::AI_PUSH;
const FLOOR_REACH: f32 = 2000.0;
const FLOOR_BALL: f32 = 0.3;
/// What `floor` answers where there is nothing below. [stand-in]
pub const NO_FLOOR: f32 = -10_000.0;
/// The ring of wall rays: how many directions and at which fractions of the height. [stand-in]
const WALL_RAYS: usize = 12;
const WALL_HEIGHTS: [f32; 3] = [0.35, 0.6, 0.9];

pub struct LevelWorld {
    pub world: StaticWorld,
    /// The masks of each placed primitive, in `world.primitives` order.
    pub masks: Vec<Masks>,
    /// Each primitive's Instance, as its number in `by_owner` (None: no Instance places it).
    prim_owner: Vec<Option<usize>>,
    /// Each Instance that owns a primitive, by its key, as its number.
    owners: HashMap<Owner, usize>,
    /// The primitives of each Instance (indices into `world.primitives`), by number.
    by_owner: Vec<Vec<usize>>,
    /// Set by `remove_owner`, by number: that Instance's primitives are gone from every query. An atomic, as the
    /// level is shared (`Arc`) and removed while it is read.
    gone: Vec<AtomicBool>,
    /// The mission's nav mesh (`game/src/main.rs` `build_nav`), which the climb's ledge search walks (`ledge_from_mesh`).
    /// None: no wall can be climbed on this level.
    nav: Option<Arc<NavMesh>>,
}

impl std::fmt::Debug for LevelWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LevelWorld({} primitives)", self.world.primitives.len())
    }
}

impl LevelWorld {
    /// `zone` is a path under the install, such as `levels/shanghai_01/zone.str`.
    pub fn load(game_dir: &Path, zone: &str) -> Result<Self, String> {
        let presets = Presets::load(game_dir)?;
        let world = StaticWorld::load(&game_dir.join(zone))?;
        Ok(Self::new(world, &presets))
    }

    /// Several packs as one level (a mission's `streamFiles`: `crate::mission`).
    pub fn load_packs(game_dir: &Path, packs: &[std::path::PathBuf]) -> Result<Self, String> {
        let presets = Presets::load(game_dir)?;
        Ok(Self::new(StaticWorld::load_packs(packs)?, &presets))
    }

    pub fn new(world: StaticWorld, presets: &Presets) -> Self {
        let masks = world.primitives.iter().map(|p| presets.resolve(p.mask_preset, p.send_mask, p.receive_mask)).collect();
        let mut owners: HashMap<Owner, usize> = HashMap::new();
        let mut by_owner: Vec<Vec<usize>> = Vec::new();
        let prim_owner = world
            .primitives
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let key = p.owner?;
                let n = *owners.entry(key).or_insert_with(|| {
                    by_owner.push(Vec::new());
                    by_owner.len() - 1
                });
                by_owner[n].push(i);
                Some(n)
            })
            .collect();
        let gone = (0..by_owner.len()).map(|_| AtomicBool::new(false)).collect();
        Self { world, masks, prim_owner, owners, by_owner, gone, nav: None }
    }

    /// Gives the level its nav mesh, so that the climb can find walls on it.
    pub fn with_nav(mut self, nav: Option<Arc<NavMesh>>) -> Self {
        self.nav = nav;
        self
    }

    /// The climb's ledge search (`notes\climbing.md` "What starts a climb"): walk the nav mesh from `from` along
    /// `toward` for `reach`, edge by edge, until the far side's top is above `from.z`; that edge is the wall. A far
    /// top flagged canNotClimb, or an open top, is no wall: None. [game] 006f7130 reports the
    /// near/far cell floors as the bottom/top. Collision probes do not choose these heights:
    /// a facade detail can lie below the authored silhouette's roof, as on the escort.
    pub fn ledge_from_mesh(&self, mesh: &NavMesh, from: Vec3, toward: Vec2, reach: f32) -> Option<Ledge> {
        self.ledge_from_mesh_query(mesh, from, toward, reach, ClimbQueryContext::default()).ledge
    }

    /// Cached-cell form of the climb sweep. The player/AI selector controls the original
    /// ceiling gate, and the returned context must be retained even after a miss.
    pub fn ledge_from_mesh_query(
        &self,
        mesh: &NavMesh,
        from: Vec3,
        toward: Vec2,
        reach: f32,
        mut context: ClimbQueryContext,
    ) -> ClimbQueryResult {
        let dir = toward.normalize_or_zero();
        if dir == Vec2::ZERO {
            return ClimbQueryResult { ledge: None, context };
        }
        let origin = Vec2::new(from.x, from.y);
        let Some(mut tri) = mesh.locate_seeded(from, context.triangle) else {
            // [game] 006f7130 zeroes the output first and returns before writing
            // output[0x1f] if the seed walk cannot locate the query point.
            context.triangle = None;
            return ClimbQueryResult { ledge: None, context };
        };
        // [game] output[0x1f] is written once from this initial triangle (006f7130:30).
        // The caller copies that value to owner+0x398 even when the sweep finds no ledge.
        context.triangle = Some(tri);
        // [game] 006f7130 31..34, 68: compare the far floor to the
        // greater of the original start height and its containing cell's floor.
        let start_z = mesh.tri_floor(tri).max(from.z);
        let mut passed = 0.0f32;
        let mut visited = Vec::new();
        for _ in 0..10_000 {
            // [game] 006f7130 marks the current triangle with the query stamp
            // and excludes stamped neighbours (cleaned 40..53). Comparing
            // distances alone can select the reverse of an already crossed
            // edge because its reversed arithmetic rounds slightly differently.
            visited.push(tri);
            // The edge this ray leaves the triangle by: the first crossing after the last one passed.
            let mut exit: Option<(f32, usize)> = None;
            for k in 0..3 {
                if mesh.neighbour(tri,k).is_some_and(|far|visited.contains(&far)) {
                    continue;
                }
                let (a, b) = edge_xy(mesh, tri, k);
                if let Some(s) = nav_segment_hit(origin, dir, reach, a, b) {
                    if s >= passed && exit.is_none_or(|(best, _)| s < best) {
                        exit = Some((s, k));
                    }
                }
            }
            let Some((s,k)) = exit else {
                return ClimbQueryResult { ledge: None, context };
            };
            if s > reach {
                return ClimbQueryResult { ledge: None, context };
            }
            let Some(far) = mesh.neighbour(tri, k) else {
                return ClimbQueryResult { ledge: None, context };
            };
            let far_floor = mesh.tri_floor(far);
            if far_floor.abs() >= 1e30 {
                return ClimbQueryResult { ledge: None, context };
            }
            if far_floor <= start_z {
                passed = s;
                tri = far;
                continue;
            }
            if !context.selector && !same_nav_layer(mesh.tri_ceiling(tri), mesh.tri_ceiling(far)) {
                passed = s;
                tri = far;
                continue;
            }
            let (a, b) = edge_xy(mesh, tri, k);
            let normal = side_normal(mesh, tri, a, b);
            let hit = origin + dir * s;
            if mesh.tri_flags(far) as u32 & FLAG_CAN_NOT_CLIMB != 0 {
                return ClimbQueryResult { ledge: None, context };
            }
            // [game] 006f7130 99..102: the candidate's near/far floors,
            // not a decorative surface found by a physics probe.
            // [game] 006f7130 returns the candidate's authored near/far floors;
            // 007d6b70 separately lowers the frame by max(initialFloor-startZ, 0).
            let bottom = mesh.tri_floor(tri);
            let top = far_floor;
            let height_adjustment = start_z - from.z;
            // [game] 006f7130: walk adjoining half-edges through the low-side fan,
            // continuing while their normal agrees by >0.98; retain the first
            // corner normal at either end for 007d6b70's radius+2 extension.
            let (left, left_corner) = wall_end(mesh, tri, k, hit, normal, start_z, context.selector, false);
            let (right, right_corner) = wall_end(mesh, tri, k, hit, normal, start_z, context.selector, true);
            return ClimbQueryResult {
                ledge: Some(Ledge { hit, normal, bottom, top, height_adjustment, left, right, left_corner, right_corner, extra: mesh.tri_extra(far) }),
                context,
            };
        }
        ClimbQueryResult { ledge: None, context }
    }

    /// Whether primitive `i` belongs to an Instance that `remove_owner` has removed.
    fn is_gone(&self, i: usize) -> bool {
        self.prim_owner[i].is_some_and(|n| self.gone[n].load(Ordering::Relaxed))
    }

    /// A primitive's +0x9e word (its receive mask).
    pub fn receive_mask(&self, primitive: usize) -> u16 {
        self.masks[primitive].receive
    }

    fn takes(&self, hit: &WorldHit, mask: u16) -> bool {
        !self.world.primitives[hit.primitive].released && !self.is_gone(hit.primitive) && self.masks[hit.primitive].send & mask != 0
    }

    /// Takes one Instance's primitives out of every query, for good: a destroyed scripted Instance stops being solid.
    /// No grid is rebuilt; the grid keeps the primitives as candidates and `takes` skips them. Returns how many
    /// primitives the Instance holds (0 when no primitive has this owner). [stand-in] what the game removes on death is
    /// not read (`notes\mission-level-instances.md`); this removes only the collision.
    pub fn remove_owner(&self, owner: Owner) -> usize {
        match self.owners.get(&owner) {
            Some(&n) => {
                self.gone[n].store(true, Ordering::Relaxed);
                self.by_owner[n].len()
            }
            None => 0,
        }
    }

    /// Whether the Instance has a solid primitive left: one not released (a DamageLink or DetachLink piece is never
    /// solid here) and not removed by `remove_owner`. False for an Instance with no primitives.
    pub fn owner_is_solid(&self, owner: Owner) -> bool {
        self.owners.get(&owner).is_some_and(|&n| {
            !self.gone[n].load(Ordering::Relaxed) && self.by_owner[n].iter().any(|&i| !self.world.primitives[i].released)
        })
    }

    /// The number of Instances that own at least one primitive.
    pub fn owner_count(&self) -> usize {
        self.owners.len()
    }

    /// The nearest hit of the segment against one Instance's solid primitives: the ones not released and not removed.
    /// The round's mask is not known, so every solid primitive of the Instance takes the round. [stand-in]
    pub fn ray_of_owner(&self, owner: Owner, p0: Vec3, p1: Vec3) -> Option<WorldHit> {
        let &n = self.owners.get(&owner)?;
        if self.gone[n].load(Ordering::Relaxed) {
            return None;
        }
        self.world
            .ray_cast_some(&self.by_owner[n], p0, p1)
            .into_iter()
            .filter(|h| !self.world.primitives[h.primitive].released)
            .min_by(|a, b| a.lambda.total_cmp(&b.lambda))
    }

    /// The nearest hit of the segment among the bodies the mask takes. A hit that is a
    /// contact at the start (the segment begins inside the shape) counts, at lambda 0.
    pub fn ray(&self, p0: Vec3, p1: Vec3, mask: u16) -> Option<WorldHit> {
        self.world.ray_cast(p0, p1).into_iter().filter(|h| self.takes(h, mask)).min_by(|a, b| a.lambda.total_cmp(&b.lambda))
    }

    /// A ball of `radius` moved from p0 to p1: the nearest body the mask takes that it
    /// touches ([game] the sphere tests, `gjk_swept`). `lambda` is how far its centre got,
    /// in metres; a ball that starts touching something stops at 0.
    pub fn sweep(&self, p0: Vec3, p1: Vec3, radius: f32, mask: u16) -> Option<WorldHit> {
        self.world.sphere_cast(p0, p1, radius).into_iter().filter(|h| self.takes(h, mask)).min_by(|a, b| a.lambda.total_cmp(&b.lambda))
    }

    /// As `sweep`, without what the ball touches where it starts.
    pub fn sweep_from_outside(&self, p0: Vec3, p1: Vec3, radius: f32, mask: u16) -> Option<WorldHit> {
        self.world
            .sphere_cast(p0, p1, radius)
            .into_iter()
            .filter(|h| !h.contact && self.takes(h, mask))
            .min_by(|a, b| a.lambda.total_cmp(&b.lambda))
    }

    /// As `ray`, without the shapes the segment starts inside.
    pub fn ray_from_outside(&self, p0: Vec3, p1: Vec3, mask: u16) -> Option<WorldHit> {
        self.world
            .ray_cast(p0, p1)
            .into_iter()
            .filter(|h| !h.contact && self.takes(h, mask))
            .min_by(|a, b| a.lambda.total_cmp(&b.lambda))
    }

    /// One step of the avatar's pencil, feet at `from` going to `*pos`, through the game's contact pass
    /// (`physics_contact`, `notes\physics.md` 1.5): the pair cells against every primitive near the path,
    /// the hits in order of t, commit and slide, the approaching velocity cut. A committed step ends at the
    /// slid end (`*pos`); otherwise `*pos` is left as the caller integrated it. Returns whether a contact
    /// that is not a floor responded (a wall).
    /// [game] the pencil's sizes (006e7e90), its centre 63/121 of the height above the feet and the floor
    /// reach 24/121 (006e7b60), the pass. [assumed] the avatar's holder masks are `CHARACTER_MASK` (0x45)
    /// in both words, and a primitive's +0x9c / +0x9e are its send / receive masks (only the floor rule's
    /// GROUND bit tells them apart). [stand-in] the start pose is upright with no rotation, and the end
    /// is the caller's integrated position, not the body's own prediction (006ddad0).
    pub fn move_pencil(&self, from: Vec3, pos: &mut Vec3, vel: &mut Vec3, radius: f32, height: f32) -> bool {
        use crate::pair_sweep::PairShape;
        use crate::physics_contact::{contact_pass, AvatarFloor, MovingBody, IDENTITY};
        let gap_part = height * 0.45454544;
        let cylinder = height - gap_part;
        let h = cylinder * 0.5;
        let tip = gap_part * 0.5;
        let shape = PairShape::Pencil { r: radius, h, a: h + tip, c: h / (tip * tip + radius * radius).sqrt() };
        let centre = h + cylinder * 0.45454544;
        let floor = AvatarFloor { centre_above_feet: centre, floor_reach: 0.8 * (cylinder * 0.45454544), wide_tolerance: false };
        let place = |feet: Vec3| {
            let mut m = IDENTITY;
            m[9] = feet.x;
            m[10] = feet.y;
            m[11] = feet.z + centre;
            m
        };
        let reach = Vec3::new(radius, radius, h + tip) + Vec3::splat(RAY_TOL_PAIR);
        let (lo, hi) = (from.min(*pos) + Vec3::Z * centre - reach, from.max(*pos) + Vec3::Z * centre + reach);
        let mut nodes = self.world.pair_nodes(lo, hi, |i| {
            let open = !self.world.primitives[i].released && !self.is_gone(i);
            open.then(|| [self.masks[i].send, self.masks[i].receive])
        });
        let mut body = MovingBody::new(shape, place(from), place(*pos), *vel, [CHARACTER_MASK; 2], Some(floor));
        let res = contact_pass(&mut body, &mut nodes);
        if res.committed {
            *pos = Vec3::new(body.predicted[9], body.predicted[10], body.predicted[11] - centre);
        }
        *vel = body.velocity;
        res.contacts.iter().any(|c| c.responded && !c.floor)
    }

    /// One world step of the robot on foot (`physics_avatar::step`, `notes\physics.md` 1.9): gravity, the
    /// control step's planes, the prediction, the contact pass with floors and walls alike, the refresh of
    /// the contacts `body` keeps, the solver and the pose pass. `feet` and `velocity` are where the step
    /// starts (the velocity as the mode left it), `controlled` what the mode's control asks along x and y.
    /// Then the owner's ground test (008524b0, `physics_avatar::ground_test`) over the kept contacts, as the
    /// next update starts it, and the next ground probe (00853420): straight down 10 m from 1.25 above the
    /// feet, mask 0x41 (the player's), answered at once ([game] the world step answers it before the next
    /// test reads it). `facing` is the character's forward row (the cone test's axis), `in_move_mode`
    /// MoveMode is the mode (STEP+0x3e4).
    /// [game] the pass, as `move_pencil`'s. [stand-in] the nodes are every primitive whose box meets the
    /// step's swept box (not the game's broad phase), rebuilt each step. [assumed] the probe's distance
    /// (result record [10]) is in metres; the probe starts at the feet (the character transform's position);
    /// the static world holds nothing with a body, so the probe never holds a handle.
    /// `bodies` are the other characters' and cars' snapshots: they take part as moving bodies, and this
    /// body takes its own side of each pair (`physics_avatar::BodySnapshot`).
    pub fn step_avatar(&self, body: &mut crate::physics_avatar::AvatarBody, req: &crate::physics_avatar::AvatarRequest,
        bodies: &[crate::physics_avatar::BodySnapshot]) -> crate::physics_avatar::FeetStep {
        use crate::physics_avatar::{ground_test, pencil_avatar, step, AvatarInput, FeetStep, GroundOwner, PEER_NODE, PROBE_LENGTH, PROBE_LIFT};
        let crate::physics_avatar::AvatarRequest { feet, velocity, controlled, gravity, radius, height, facing, in_move_mode, ai, id, dt, .. } = *req;
        let (shape, floor) = pencil_avatar(radius, height);
        let up = Vec3::Z * floor.centre_above_feet;
        let centre = feet + up;
        let guess = Vec3::new(controlled.x, controlled.y, velocity.z - gravity * dt) * dt;
        let reach = Vec3::new(radius, radius, height * 0.5) + Vec3::splat(RAY_TOL_PAIR + 0.5);
        let (lo, hi) = (centre.min(centre + guess) - reach, centre.max(centre + guess) + reach);
        let nodes = self.world.pair_nodes(lo, hi, |i| {
            let open = !self.world.primitives[i].released && !self.is_gone(i);
            open.then(|| [self.masks[i].send, self.masks[i].receive])
        });
        let masks = if ai { AI_CHARACTER_MASK } else { CHARACTER_MASK };
        let inp = AvatarInput { shape, floor, masks: [masks; 2], centre, velocity, controlled, gravity, damping: AVATAR_DAMPING,
            inv_mass: req.inv_mass(), id, dt };
        // [stand-in for the broad phase] the bodies within 50 m (more than any two bodies' reach in a step)
        let near: Vec<_> = bodies.iter().filter(|b| (b.centre - centre).length() < 50.0 + velocity.length() * dt).copied().collect();
        let out = step(body, &inp, nodes, &near, |node, from, to| {
            self.world.ray_cast_some(&[node], from, to).into_iter().min_by(|a, b| a.lambda.total_cmp(&b.lambda)).map(|h| (h.point, h.normal))
        });
        let owner = GroundOwner {
            origin: out.centre.to_array(),
            up: [0.0, 0.0, 1.0],
            gap: floor.centre_above_feet,
            velocity: out.velocity.to_array(),
            cone: [facing.x, facing.y, 0.0],
            in_move_mode,
            held: false,
            r1: floor.floor_reach,
            r2: height,
        };
        let mut ground = body.ground;
        let lists: Vec<(bool, usize, &[crate::physics_manifold::Slot; 5])> = body
            .contacts
            .iter()
            .zip(&body.owners)
            .map(|(k, o)| {
                let send = if o.node >= PEER_NODE {
                    bodies.iter().find(|b| PEER_NODE + b.id as usize == o.node).map_or(0, |b| b.masks[0])
                } else {
                    self.masks[o.node].send
                };
                (send & bits::SLIPPERY != 0, k.count, &k.slots)
            })
            .collect();
        let v = Vec3::from_array(ground_test(&mut ground, &owner, lists));
        let feet = out.centre - up;
        let from = feet + Vec3::Z * PROBE_LIFT;
        let hit = self.ray(from, from - Vec3::Z * PROBE_LENGTH, if ai { AI_PROBE_MASK } else { PLAYER_PROBE_MASK });
        ground.probe_done = true;
        (ground.probe_distance, ground.probe_normal) = match hit {
            Some(h) => ((from - h.point).length(), h.normal.to_array()),
            None => (f32::MAX, ground.probe_normal),
        };
        body.ground = ground;
        FeetStep { feet, velocity: v, wall: out.wall, standing: ground.grounded }
    }

    /// One step of the car's chassis through the contact pass: the cube of half extents `half` at `centre`,
    /// turned `turn` now and `next` at the end, moving by `*velocity` for `dt`. Returns the end centre (the
    /// slid end when the pass committed, else centre + v dt with the cut velocity: the pose pass) and the
    /// contacts that responded, as the chassis' contacts (key = primitive index + 1, depth 0).
    /// [game] the shape: 00851870 -> vehicle_avatar_create 00853a10 -> cuboid_avatar_shape 006e81e0 build a
    /// `CollisionCube` of half extents (width, length, height) from avatarDescArray[1] (descriptor +0xc,
    /// +0x10, +4; the loader 00726710), centred on the holder origin, which is the root plus the avatar
    /// offset (0, 0, offsetZ): `DriveTuning::body_half` and the chassis centre. Its floor heights are
    /// wrapper +0x54 = 0.5 and +0x50 = 0.9 of the half height (006e81e0). The friction term is 0 for an
    /// avatar holder (bit 0x1000; 006e2c10 line 246), which the car's cube is.
    /// [assumed] the car's holder masks are `CHARACTER_MASK` in both words, as the robot's (A-PH8), and it
    /// takes the floor rule, its root being the robot's (A-PH9). [stand-in] the start is the port's own
    /// chassis state and the end its velocity integration, not the body's prediction (006ddad0); the
    /// angular arm terms (006e92f0 / 006e8f00) and the persistent-contact solver are not run.
    pub fn move_chassis(&self, centre: Vec3, turn: glam::Quat, next: glam::Quat, half: Vec3, velocity: &mut Vec3, angular: Vec3, dt: f32)
        -> (Vec3, Vec<crate::vehicle::Contact>) {
        use crate::pair_sweep::PairShape;
        use crate::physics_contact::{contact_pass, AvatarFloor, MovingBody};
        let pose = |q: glam::Quat, o: Vec3| {
            let (x, y, z) = (q * Vec3::X, q * Vec3::Y, q * Vec3::Z);
            [x.x, x.y, x.z, y.x, y.y, y.z, z.x, z.y, z.z, o.x, o.y, o.z]
        };
        let end = centre + *velocity * dt;
        let extent = |q: glam::Quat| (q * Vec3::X).abs() * half.x + (q * Vec3::Y).abs() * half.y + (q * Vec3::Z).abs() * half.z;
        let reach = extent(turn).max(extent(next)) + Vec3::splat(RAY_TOL_PAIR);
        let (lo, hi) = (centre.min(end) - reach, centre.max(end) + reach);
        let mut nodes = self.world.pair_nodes(lo, hi, |i| {
            let open = !self.world.primitives[i].released && !self.is_gone(i);
            open.then(|| [self.masks[i].send, self.masks[i].receive])
        });
        let floor = AvatarFloor { centre_above_feet: half.z * 0.5, floor_reach: half.z * 0.9, wide_tolerance: false };
        let mut body = MovingBody::new(PairShape::Cube { half }, pose(turn, centre), pose(next, end), *velocity, [CHARACTER_MASK; 2], Some(floor));
        let res = contact_pass(&mut body, &mut nodes);
        let out = res.end(&body, dt);
        *velocity = body.velocity;
        let contacts = res.contacts.iter().filter(|c| c.responded).map(|c| crate::vehicle::Contact {
            key: c.id as u32 + 1,
            fraction: c.record.t,
            point: c.record.point,
            normal: c.record.normal,
            depth: 0.0,
            surface: melee::surface::DEFAULT,
            // [game 006e2c10 line 242, disasm 006e3172-006e34f9] the chassis point's velocity at this hit: the linear
            // velocity when it was taken (`PassContact::velocity`) plus w x r, the arm r from A's world origin AFTER
            // the hit's commit (lines 143-145 run before the generate and before body_world_position 006e9460), the
            // record's A pose, as `step_chassis` maps it.
            velocity: c.velocity + angular.cross(c.record.point - Vec3::new(c.record.a_at[9], c.record.a_at[10], c.record.a_at[11])),
        }).collect();
        (Vec3::new(out[9], out[10], out[11]), contacts)
    }

    /// One world step of the car's chassis with its kept contacts (`physics_avatar::step_chassis`,
    /// `notes\physics.md` 1.11): the pass, the refresh, the solver with the cube's arms, the pose pass. The
    /// responded contacts come back as the chassis' contacts, as `move_chassis` gives them.
    /// [stand-in] the nodes are the primitives whose boxes meet the step's box, rebuilt each step.
    pub fn step_chassis(&self, body: &mut crate::physics_avatar::AvatarBody, inp: &crate::physics_avatar::ChassisInput, ai: bool,
        bodies: &[crate::physics_avatar::BodySnapshot]) -> (crate::physics_avatar::ChassisOutput, Vec<crate::vehicle::Contact>) {
        use crate::physics_avatar::PEER_NODE;
        let (centre, half) = (inp.centre, inp.half);
        let end = centre + inp.velocity * inp.dt;
        let extent = |q: glam::Quat| (q * Vec3::X).abs() * half.x + (q * Vec3::Y).abs() * half.y + (q * Vec3::Z).abs() * half.z;
        let reach = extent(inp.turn).max(extent(inp.next)) + Vec3::splat(RAY_TOL_PAIR + 0.5);
        let (lo, hi) = (centre.min(end) - reach, centre.max(end) + reach);
        let nodes = self.world.pair_nodes(lo, hi, |i| {
            let open = !self.world.primitives[i].released && !self.is_gone(i);
            open.then(|| [self.masks[i].send, self.masks[i].receive])
        });
        let masks = if ai { AI_CHARACTER_MASK } else { CHARACTER_MASK };
        // [stand-in for the broad phase] the bodies within 50 m
        let near: Vec<_> = bodies.iter().filter(|b| (b.centre - centre).length() < 50.0 + inp.velocity.length() * inp.dt).copied().collect();
        let out = crate::physics_avatar::step_chassis(body, inp, [masks; 2], nodes, &near, |node, from, to| {
            self.world.ray_cast_some(&[node], from, to).into_iter().min_by(|a, b| a.lambda.total_cmp(&b.lambda)).map(|h| (h.point, h.normal))
        });
        // a body's contact keys count down from u32::MAX (actor ids), the level's up from 1 (primitives)
        let contacts = out.pass.contacts.iter().filter(|c| c.responded).map(|c| crate::vehicle::Contact {
            key: if c.id >= PEER_NODE { u32::MAX - (c.id - PEER_NODE) as u32 } else { c.id as u32 + 1 },
            fraction: c.record.t,
            point: c.record.point,
            normal: c.record.normal,
            depth: 0.0,
            surface: melee::surface::DEFAULT,
            // The chassis point's velocity at THIS hit: the linear velocity the pass had then, plus
            // the turn's part at the point about the chassis's centre at the hit (v + w x r).
            // [game 006e2c10 line 242]
            velocity: c.velocity + inp.angular.cross(c.record.point - Vec3::new(c.record.a_at[9], c.record.a_at[10], c.record.a_at[11])),
        }).collect();
        (out, contacts)
    }

    /// Sweep the game's upright pencil through static level geometry and retain only
    /// bodies accepted by `mask`. Hits are ordered by travel distance. [game] the
    /// pencil dimensions/support; [stand-in] the narrow phase is the existing ray-GJK
    /// Minkowski cast, not the game's swept-pair dispatcher/contact manifold.
    pub fn pencil_contacts(&self, p0: Vec3, p1: Vec3, radius: f32, height: f32, mask: u16) -> Vec<WorldHit> {
        let mut hits: Vec<_> = self.world.pencil_cast(p0, p1, radius, height)
            .into_iter().filter(|h| self.takes(h, mask)).collect();
        hits.sort_by(|a, b| a.lambda.total_cmp(&b.lambda));
        hits
    }
}

/// The next wall around an endpoint, reached through the near-side triangle fan.
/// [game] 006f7130 (left 006f775c, right after 006f7a15): cross low neighbours,
/// stop at the first higher floor, reject an open or canNotClimb far triangle.
/// The selector carries the original param_5 ceiling gate through the endpoint fan.
fn adjacent_wall(mesh: &NavMesh, mut tri: usize, mut k: usize, z: f32, selector: bool, right: bool) -> Option<(usize, usize)> {
    let start = tri;
    for _ in 0..mesh.tri_count() {
        let next = (k + if right { 1 } else { 2 }) % 3;
        let (far, twin) = mesh.edge_twin(tri, next)?;
        let floor = mesh.tri_floor(far);
        if floor > z {
            let layer_ok = selector || same_nav_layer(mesh.tri_ceiling(tri), mesh.tri_ceiling(far));
            return (floor.abs() < 1e30 && mesh.tri_flags(far) as u32 & FLAG_CAN_NOT_CLIMB == 0 && layer_ok).then_some((tri, next));
        }
        (tri, k) = (far, twin);
        if tri == start {
            return None;
        }
    }
    None
}

/// An endpoint's distance from the sweep hit, and the first corner's outward normal.
/// [game] 006f7130 compares each adjacent normal with the original wall (>0.98),
/// and writes Euclidean distance to the endpoint when that chain ends.
fn wall_end(mesh: &NavMesh, mut tri: usize, mut k: usize, hit: Vec2, normal: Vec2, z: f32, selector: bool, right: bool) -> (f32, Option<Vec2>) {
    let first = (tri, k);
    for _ in 0..mesh.tri_count().saturating_mul(3) {
        let (a, b) = edge_xy(mesh, tri, k);
        let end = if right { b } else { a };
        let distance = (end - hit).length();
        let Some((next, edge)) = adjacent_wall(mesh, tri, k, z, selector, right) else {
            return (distance, None);
        };
        let (c, d) = edge_xy(mesh, next, edge);
        let corner = side_normal(mesh, next, c, d);
        if corner.dot(normal) <= 0.98 {
            return (distance, Some(corner));
        }
        if (next, edge) == first {
            return (distance, None);
        }
        (tri, k) = (next, edge);
    }
    let (a, b) = edge_xy(mesh, tri, k);
    let end = if right { b } else { a };
    ((end - hit).length(), None)
}

/// `a x b` in the ground plane.
fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// The player query keeps the same authored ceiling layer, unless the far cell
/// has no ceiling. [game] 006f7130 lines 68-70; AI (`selector`) bypasses this.
fn same_nav_layer(near_ceiling: f32, far_ceiling: f32) -> bool {
    far_ceiling == f32::MAX || near_ceiling == far_ceiling
}

/// Segment intersection used by the original climb sweep (`nav_segment_intersect`,
/// 006fabb0): parallel threshold 0.001 and 0.01 cross-unit endpoint slack. Returns
/// distance along the normalized query direction, clamped to its finite segment.
fn nav_segment_hit(origin: Vec2, direction: Vec2, reach: f32, a: Vec2, b: Vec2) -> Option<f32> {
    let end = origin + direction * reach;
    // [game] 006fabb0 first rejects disjoint inclusive x and y bounding boxes;
    // the later 0.01 tolerance belongs to cross products, not these bounds.
    if max2(origin.x, end.x) < min2(a.x, b.x) || max2(a.x, b.x) < min2(origin.x, end.x)
        || max2(origin.y, end.y) < min2(a.y, b.y) || max2(a.y, b.y) < min2(origin.y, end.y)
    {
        return None;
    }

    let d1 = end - origin;
    let d2 = b - a;
    let aq = a - origin;
    let den = cross(d1, d2);
    let t_num = cross(aq, d2);
    let u_num = cross(aq, d1);
    // [game] 006fabb0 checks both signed numerators against [0, den] with
    // exactly 0.01 cross-unit slack, accounting for a negative determinant.
    let (t_in_range, u_in_range) = if den > 0.0 {
        (t_num >= -0.01 && t_num <= den + 0.01, u_num >= -0.01 && u_num <= den + 0.01)
    } else {
        (t_num <= 0.01 && t_num >= den - 0.01, u_num <= 0.01 && u_num >= den - 0.01)
    };
    if !t_in_range || !u_in_range {
        return None;
    }
    // [game] parallel/near-parallel segments intersect only in the exact
    // collinear case after both numerator range checks (006fabb0:86-107).
    if den.abs() < 0.001 {
        return (t_num == 0.0).then_some(0.0);
    }
    Some((t_num / den).clamp(0.0, 1.0) * reach)
}

fn min2(a: f32, b: f32) -> f32 { a.min(b) }
fn max2(a: f32, b: f32) -> f32 { a.max(b) }

/// A mesh edge's two ends on the ground plane.
fn edge_xy(mesh: &NavMesh, tri: usize, k: usize) -> (Vec2, Vec2) {
    let (a, b) = mesh.edge_points(tri, k);
    (Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1]))
}

/// The unit normal of a mesh edge of triangle `tri`, pointing out of the wall toward `tri`'s side.
fn side_normal(mesh: &NavMesh, tri: usize, a: Vec2, b: Vec2) -> Vec2 {
    let e = b - a;
    let mut n = Vec2::new(-e.y, e.x).normalize_or_zero();
    let [ix, iy] = mesh.tri_incenter(tri);
    if n.dot(Vec2::new(ix, iy) - (a + b) * 0.5) < 0.0 {
        n = -n;
    }
    n
}

impl World for LevelWorld {
    fn ledge_query(&self, from: Vec3, toward: Vec2, reach: f32, context: ClimbQueryContext) -> ClimbQueryResult {
        match &self.nav {
            Some(mesh) => self.ledge_from_mesh_query(mesh, from, toward, reach, context),
            None => ClimbQueryResult { ledge: None, context },
        }
    }

    /// The climb's wall on a level: the nav mesh's walk (`ledge_from_mesh`). None without a mesh.
    fn ledge(&self, from: Vec3, toward: Vec2, reach: f32) -> Option<Ledge> {
        let mesh = self.nav.clone()?;
        self.ledge_from_mesh(&mesh, from, toward, reach)
    }

    fn wheel_ray(&self, from: Vec3, to: Vec3) -> Option<GroundHit> {
        self.ray_from_outside(from, to, CHARACTER_MASK)
            .map(|h| GroundHit { point: h.point, normal: h.normal, surface: melee::surface::DEFAULT,
                object_mask: self.masks[h.primitive].receive })
    }

    /// [stand-in] A small ball let down from a step above the feet (the game's sphere test,
    /// `gjk_swept`; the probe itself is this port's): the floor is where its underside
    /// stops. A ball does not fall into the cracks between slabs as a ray does, and it
    /// rounds a kerb's edge.
    fn floor(&self, x: f32, y: f32, z: f32) -> f32 {
        let top = z + STEP + FLOOR_BALL;
        self.sweep_from_outside(Vec3::new(x, y, top), Vec3::new(x, y, z - FLOOR_REACH), FLOOR_BALL, CHARACTER_MASK)
            .map_or(NO_FLOOR, |h| top - h.lambda - FLOOR_BALL)
    }

    /// [stand-in] A ring of balls pushed from the axis outward, above step height: each one
    /// that stops short of the radius pushes him back by what is missing. The balls are wide
    /// enough to leave no gap between neighbours at the rim, so a pole is not missed.
    fn push_out(&self, pos: &mut Vec3, radius: f32, height: f32) -> bool {
        let ball = radius * (std::f32::consts::PI / WALL_RAYS as f32).sin() * 1.05;
        let mut hit = false;
        for &fraction in &WALL_HEIGHTS {
            let z = pos.z + (height * fraction).max(STEP + ball + 0.05);
            for i in 0..WALL_RAYS {
                let angle = i as f32 / WALL_RAYS as f32 * std::f32::consts::TAU;
                let dir = Vec2::new(angle.cos(), angle.sin());
                let from = Vec3::new(pos.x, pos.y, z);
                let reach = radius - ball;
                let Some(h) = self.sweep_from_outside(from, from + dir.extend(0.0) * reach, ball, CHARACTER_MASK) else { continue };
                let short = reach - h.lambda;
                if short > 1e-4 {
                    pos.x -= dir.x * short;
                    pos.y -= dir.y * short;
                    hit = true;
                }
            }
        }
        hit
    }

    fn move_pencil(&self, from: Vec3, pos: &mut Vec3, vel: &mut Vec3, radius: f32, height: f32) -> bool {
        LevelWorld::move_pencil(self, from, pos, vel, radius, height)
    }

    fn step_avatar(&self, body: &mut crate::physics_avatar::AvatarBody, req: &crate::physics_avatar::AvatarRequest) -> Option<crate::physics_avatar::FeetStep> {
        Some(LevelWorld::step_avatar(self, body, req, &[]))
    }
    fn step_chassis(&self, body: &mut crate::physics_avatar::AvatarBody, inp: &crate::physics_avatar::ChassisInput, ai: bool)
        -> Option<(crate::physics_avatar::ChassisOutput, Vec<crate::vehicle::Contact>)> {
        Some(LevelWorld::step_chassis(self, body, inp, ai, &[]))
    }
    fn move_chassis(&self, centre: Vec3, turn: glam::Quat, next: glam::Quat, half: Vec3, velocity: &mut Vec3, angular: Vec3, dt: f32)
        -> Option<(Vec3, Vec<crate::vehicle::Contact>)> {
        Some(LevelWorld::move_chassis(self, centre, turn, next, half, velocity, angular, dt))
    }

    fn chassis_pass(&self) -> bool { true }

    fn front_contact(&self, pos: Vec3, facing: Vec2, radius: f32, height: f32) -> bool {
        // [game] 008524b0 uses the pencil's contacts and a forward cone. [stand-in]
        // Our contacts come from the pencil Minkowski cast; the original contact list
        // generator is still unported.
        let step = facing.normalize_or_zero() * 0.01;
        self.pencil_contacts(pos, pos + step.extend(0.0), radius, height, CHARACTER_MASK)
            .iter().any(|h| climb::contact_ahead(h.point - pos, facing))
    }

    /// The jump's ledge catch: [game] a ball of his radius let straight down, against mask
    /// 0x5d (0087a200 / 0087dde0, `notes\character-world-probes.md`). Answers the spot under
    /// its centre at the height its underside stops.
    fn drop_onto(&self, from: Vec3, to_z: f32, radius: f32) -> Option<Vec3> {
        self.sweep_from_outside(from, Vec3::new(from.x, from.y, to_z), radius, LEDGE_MASK)
            .map(|h| Vec3::new(from.x, from.y, from.z - h.lambda - radius))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision_world::{WorldPrimitive, WorldShape};
    use crate::gjk::Sphere;
    use std::path::PathBuf;

    #[test]
    fn nav_segment_kernel_keeps_aabb_and_parallel_collinear_rules_separate() {
        assert_eq!(nav_segment_hit(Vec2::ZERO, Vec2::X, 10.0, Vec2::new(4.0, 0.0), Vec2::new(6.0, 0.0)), Some(0.0));
        assert_eq!(nav_segment_hit(Vec2::ZERO, Vec2::X, 10.0, Vec2::new(10.0, -1.0), Vec2::new(10.0, 1.0)), Some(10.0));
        assert_eq!(nav_segment_hit(Vec2::ZERO, Vec2::X, 10.0, Vec2::new(4.0, 0.001), Vec2::new(6.0, 0.001)), None,
            "the original cross slack does not expand the AABB rejection");
    }

    /// A unit sphere at (x, 0, 0), owned by `owner`, sending every bit.
    fn sphere(x: f32, owner: Option<Owner>) -> WorldPrimitive {
        let id = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        WorldPrimitive {
            shape: WorldShape::Sphere(Sphere { radius: 1.0, x: [id[0], id[1], id[2], id[3], id[4], id[5], id[6], id[7], id[8], x, 0.0, 0.0], y: id }),
            owner,
            source: String::new(),
            material: 0,
            mask_preset: 0,
            send_mask: 0xffff,
            receive_mask: 0xffff,
            has_dynamics: false,
            released: false,
            centre: Vec3::new(x, 0.0, 0.0),
        }
    }

    /// An Instance's removal: its primitive stops being hit by every query, the other Instances keep theirs, and a
    /// primitive with no Instance is never removed. Synthetic world (no install needed).
    #[test]
    fn remove_owner_takes_one_instance_out_of_every_query() {
        let (a, b) = ((0, 1, 10), (0, 1, 20));
        let world = StaticWorld::from_primitives(vec![sphere(10.0, Some(a)), sphere(20.0, Some(b)), sphere(30.0, None)]);
        let level = LevelWorld::new(world, &Presets::default());
        let (p0, p1) = (Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0));
        assert_eq!(level.ray(p0, p1, 0xffff).map(|h| h.primitive), Some(0));
        assert!(level.ray_of_owner(a, p0, p1).is_some_and(|h| h.primitive == 0));
        assert!(level.owner_is_solid(a) && level.owner_is_solid(b));
        assert_eq!(level.owner_count(), 2);

        assert_eq!(level.remove_owner(a), 1);
        assert_eq!(level.ray(p0, p1, 0xffff).map(|h| h.primitive), Some(1), "a removed Instance's sphere still hit");
        assert!(level.ray_of_owner(a, p0, p1).is_none());
        assert!(!level.owner_is_solid(a));
        assert!(level.ray_of_owner(b, p0, p1).is_some_and(|h| h.primitive == 1), "another Instance lost its sphere");
        assert!(level.owner_is_solid(b));
        assert!(level.ray_from_outside(p0, p1, 0xffff).is_some_and(|h| h.primitive == 1));
        // A second removal is harmless; an unknown key removes nothing.
        assert_eq!(level.remove_owner(a), 1);
        assert_eq!(level.remove_owner((9, 9, 9)), 0);
        // The primitive with no Instance is still there.
        let far = Vec3::new(40.0, 0.0, 0.0);
        assert_eq!(level.ray(Vec3::new(25.0, 0.0, 0.0), far, 0xffff).map(|h| h.primitive), Some(2));
    }

    fn level() -> Option<LevelWorld> {
        let dir = PathBuf::from(crate::install::dir_string());
        dir.join("levels/shanghai_01/zone.str").is_file().then(|| LevelWorld::load(&dir, "levels/shanghai_01/zone.str").expect("level"))
    }

    /// The escort mission's collision with its Instances (`--mission USA_East_LTrain_A_Escort`): how many Instances own
    /// primitives, how many primitives have no Instance, how many Instances own only released (never solid) ones, and
    /// how many primitives share a stored shape at one place under two Instances (the cases the old keys merged).
    /// Needs the install; passes without it.
    #[test]
    fn escort_primitives_know_their_instance() {
        use crate::formats::missionlist::MissionList;
        use crate::formats::hash::crc32;
        use std::collections::{BTreeMap, BTreeSet};
        let dir = PathBuf::from(crate::install::dir_string());
        if !dir.join("levels").is_dir() {
            return;
        }
        let list = MissionList::load(&dir).expect("mission list");
        let Some(mission) = list.by_hash(crc32(b"USA_East_LTrain_A_Escort")) else { return };
        let packs: Vec<PathBuf> = list.pack_paths(mission).iter().map(|p| dir.join(p)).collect();
        let world = StaticWorld::load_packs(&packs).expect("escort collision");
        let owners: BTreeSet<Owner> = world.primitives.iter().filter_map(|p| p.owner).collect();
        let owned = world.primitives.iter().filter(|p| p.owner.is_some()).count();
        let level = LevelWorld::new(world, &Presets::default());
        let solid_owners = owners.iter().filter(|o| level.owner_is_solid(**o)).count();
        // Same stored shape ('source' is the Collision and the primitive) at the same centre under two Instances.
        let mut at: BTreeMap<(String, [u32; 3]), BTreeSet<Owner>> = BTreeMap::new();
        for p in &level.world.primitives {
            if let Some(o) = p.owner {
                let c = p.centre.to_array().map(f32::to_bits);
                at.entry((p.source.clone(), c)).or_default().insert(o);
            }
        }
        let same_matrix = at.values().filter(|s| s.len() > 1).count();
        println!(
            "escort collision: {} primitives, {owned} with an Instance, {} Instances own primitives ({solid_owners} with a solid one), {} owned by the mission pack; {same_matrix} stored primitives at one centre under two Instances",
            level.world.primitives.len(),
            owners.len(),
            owners.iter().filter(|o| o.0 == packs.iter().position(|p| p.ends_with("mission.str")).unwrap_or(usize::MAX)).count()
        );
        assert!(owned > 0, "no primitive knows its Instance");
    }

    #[test]
    fn shanghai_ground_is_ground_under_the_character_mask() {
        // Needs the install.
        let Some(level) = level() else { return };
        assert_eq!(level.masks.len(), level.world.primitives.len());
        // Over the level's middle: what the character mask stands on. Every such floor is
        // a body that sends one of the mask's bits, and most of the street is at z = 0.
        let (mut found, mut street) = (0, 0);
        for i in 0..20 {
            for j in 0..20 {
                let (x, y) = (-200.0 + i as f32 * 25.0, -200.0 + j as f32 * 25.0);
                let z = level.floor(x, y, 300.0);
                if z > NO_FLOOR {
                    found += 1;
                    street += (z.abs() < 0.5) as usize;
                }
            }
        }
        println!("{found} of 400 spots have a floor; {street} at street height");
        assert!(found > 300, "{found}");
        assert!(street > 100, "{street}");
    }

    /// Ground at z 0 and a platform 10 m by 10 m (x 0..10, y -5..5) 4 m high, as collision and as a nav mesh silhouette
    /// with the same outline. `flags` are the silhouette's (canNotClimb = 1). Synthetic (no install).
    fn platform_level(flags: u32) -> LevelWorld {
        platform_level_outline(flags, vec![[0.0, -5.0, 0.0], [0.0, 5.0, 0.0], [10.0, 5.0, 0.0], [10.0, -5.0, 0.0], [0.0, -5.0, 0.0]])
    }

    fn platform_level_outline(flags: u32, points: Vec<[f32; 3]>) -> LevelWorld {
        use crate::gjk_world::{Cuboid, Placed};
        use crate::navmesh::Silhouette;
        let block = |centre: Vec3, half: Vec3| WorldPrimitive {
            shape: WorldShape::Cuboid(Placed {
                shape: Cuboid { half },
                x: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, centre.x, centre.y, centre.z],
                y: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
            owner: None,
            source: String::new(),
            material: 0,
            mask_preset: 0,
            send_mask: 0xffff,
            receive_mask: 0xffff,
            has_dynamics: false,
            released: false,
            centre,
        };
        let world = StaticWorld::from_primitives(vec![
            block(Vec3::new(0.0, 0.0, -0.5), Vec3::new(500.0, 500.0, 0.5)),
            block(Vec3::new(5.0, 0.0, 2.0), Vec3::new(5.0, 5.0, 2.0)),
        ]);
        let raw_points = points.len();
        let platform = Silhouette {
            name: "platform".into(),
            points,
            base_z: 0.0,
            height: 4.0,
            replacement_height: 0.0,
            // FLT_MAX: the floor is set to the top (`apply_heights_to`); with a real ban height only the ceiling is.
            ai_ban_height: f32::MAX,
            extra_height: 0.0,
            pivot: 0.0,
            flags,
            can_not_walk_off: false,
            treat_as_dynamic: false,
            impermanent: false,
            raw_points,
            object: (0, 0),
        };
        // This fixture's flat terrain is at zero in both the nav bounds and collision.
        let mesh = NavMesh::build(Some((Vec3::new(-20.0, -20.0, 0.0), Vec3::new(20.0, 20.0, 10.0))), &[platform]).expect("test mesh builds");
        LevelWorld::new(world, &Presets::default()).with_nav(Some(Arc::new(mesh)))
    }

    #[test]
    fn seeded_climb_query_uses_the_five_mm_edge_band_without_a_side_retry() {
        let level = platform_level(0);
        let mesh = level.nav.as_ref().unwrap();
        let seed = mesh.locate(Vec3::new(-1.0, 1.0, 1.07)).unwrap();
        let result = level.ledge_from_mesh_query(
            mesh,
            Vec3::new(-0.002, 1.0, 1.07),
            Vec2::X,
            5.66,
            ClimbQueryContext { triangle: Some(seed), selector: false },
        );
        let ledge = result.ledge.expect("the classifier band and segment kernel cross the edge");
        assert!(ledge.hit.x.abs() < 0.005, "{ledge:?}");
        assert_eq!(result.context.triangle, Some(seed), "the original writes back the initial located triangle");

        let below_floor = level.ledge_from_mesh_query(
            mesh,
            Vec3::new(-3.0, 1.0, -1.0),
            Vec2::X,
            5.66,
            ClimbQueryContext { triangle: Some(seed), selector: false },
        ).ledge.expect("same wall when the query starts below the near cell floor");
        assert!(below_floor.bottom.abs() < 1e-3, "bottom remains the authored near floor: {below_floor:?}");
        assert!((below_floor.top - 4.0).abs() < 1e-3, "top remains the far cell floor: {below_floor:?}");
        assert!((below_floor.height_adjustment - 1.0).abs() < 1e-3, "the frame extends below start by the initial-floor gap: {below_floor:?}");
        let wall = climb::Wall::from_ledge(&below_floor, 0.5);
        assert!((wall.origin.z + 1.0).abs() < 1e-3 && (wall.height - 5.0).abs() < 1e-3, "frame applies the lower offset while preserving the authored floors: {wall:?}");
    }

    #[test]
    fn climb_query_ceiling_gate_is_player_only_and_keeps_the_updated_seed_on_a_miss() {
        let mut level = platform_level(0);
        let mesh = Arc::get_mut(level.nav.as_mut().unwrap()).unwrap();
        for tri in 0..mesh.tri_count() {
            mesh.set_tri_ceiling_for_test(tri, 5.0);
        }
        let near = mesh.locate(Vec3::new(-1.0, 1.0, 1.07)).unwrap();
        let far = mesh.locate(Vec3::new(1.0, 1.0, 1.07)).unwrap();
        mesh.set_tri_ceiling_for_test(far, 4.0);
        let mesh = level.nav.as_ref().unwrap();
        let from = Vec3::new(-3.0, 1.0, 1.07);
        let player = level.ledge_from_mesh_query(
            mesh, from, Vec2::X, 5.66,
            ClimbQueryContext { triangle: Some(near), selector: false },
        );
        assert!(player.ledge.is_none(), "the player cannot cross a mismatched finite ceiling layer");
        assert_eq!(player.context.triangle, Some(near), "the initial located triangle survives a candidate miss");
        let ai = level.ledge_from_mesh_query(
            mesh, from, Vec2::X, 5.66,
            ClimbQueryContext { triangle: Some(near), selector: true },
        );
        assert!(ai.ledge.is_some(), "the AI selector bypasses ceiling equality");
    }

    /// The climb's wall on a level (the nav mesh walk) against the arena's box for the same shape: every field of the
    /// ledge agrees, from the west face at two spots along it.
    #[test]
    fn mesh_ledge_matches_the_arena_box_for_the_same_shape() {
        use crate::sim::{Arena, Box3};
        let level = platform_level(0);
        let arena = Arena { boxes: vec![Box3 { min: Vec3::new(0.0, -5.0, 0.0), max: Vec3::new(10.0, 5.0, 4.0) }], ..Default::default() };
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
        for y in [1.0f32, -2.5] {
            let from = Vec3::new(-3.0, y, 1.07);
            let want = arena.ledge(from, Vec2::X, 5.66).expect("the arena's box wall");
            let got = level.ledge(from, Vec2::X, 5.66).expect("the mesh's wall");
            assert!(close(got.hit.x, want.hit.x) && close(got.hit.y, want.hit.y), "hit {got:?} vs {want:?}");
            assert!(close(got.normal.x, want.normal.x) && close(got.normal.y, want.normal.y), "normal {got:?} vs {want:?}");
            // The authored nav floors agree with this flat arena's base and top.
            assert!((got.bottom - want.bottom).abs() < 0.1, "bottom {got:?} vs {want:?}");
            assert!((got.top - want.top).abs() < 0.1, "top {got:?} vs {want:?}");
            assert!(close(got.left, want.left) && close(got.right, want.right), "left and right {got:?} vs {want:?}");
            assert!(close(got.extra, want.extra), "extra {got:?} vs {want:?}");
        }
    }

    /// A top flagged canNotClimb is no wall: None, where the same platform unflagged is a wall.
    #[test]
    fn escort_ledge_walk_does_not_reenter_a_visited_low_triangle() {
        use crate::formats::{hash::crc32, missionlist::MissionList};
        let game = PathBuf::from(crate::install::dir_string());
        if !game.join("levels").is_dir() {return;}
        let list = MissionList::load(&game).expect("missions");
        let mission = list.by_hash(crc32(b"USA_East_LTrain_A_Escort")).expect("escort");
        let packs: Vec<_> = list.pack_paths(mission).iter().map(|p|game.join(p)).collect();
        let mut silhouettes = Vec::new();
        for pack in &packs {silhouettes.extend(crate::navmesh::load(pack).expect("nav").0);}
        let mesh = Arc::new(NavMesh::build(None,&silhouettes).expect("mesh"));
        let level = LevelWorld::load_packs(&game,&packs).expect("collision").with_nav(Some(mesh));
        // Captured replay point: rounding puts the reverse edge a few micrometres
        // farther than its twin, so a distance-only guard revisits the near cell.
        for xy in [Vec2::new(-217.97708,-67.953964), Vec2::new(-218.8541,-67.761475)] {
            let ledge = level.ledge(xy.extend(16.367075),Vec2::Y,5.656854)
                .expect("a repeated low-side edge or an on-edge start must not erase the wall ahead");
            assert!(ledge.normal.dot(Vec2::NEG_Y)>0.999,
                "an ordinary low-floor diagonal became a false wall at {xy:?}: {ledge:?}");
        }
        // A local facade surface fooled the old bottom probe into reporting a
        // wall only 2.48 m high. The authored near cell has the lower base.
        let from = Vec3::new(-204.034,-67.369,16.367075);
        let ledge = level.ledge(from,Vec2::new(0.0502,0.99874),5.656854).expect("next facade");
        assert!((ledge.top-17.86849).abs()<0.001 && (ledge.bottom+18.10835).abs()<0.001,
            "authored cell heights must replace local facade probes: {ledge:?}");
        assert!(ledge.top-ledge.bottom>6.0, "the original minimum-height test must keep this wall");
    }

    #[test]
    fn a_top_flagged_can_not_climb_is_no_wall() {
        let from = Vec3::new(-3.0, 1.0, 1.07);
        assert!(platform_level(0).ledge(from, Vec2::X, 5.66).is_some());
        assert!(platform_level(FLAG_CAN_NOT_CLIMB).ledge(from, Vec2::X, 5.66).is_none());
    }

    #[test]
    fn ledge_fan_finds_a_shallow_corner_and_extends_only_that_side() {
        // The north end turns by atan(3/4); the south end turns 90 degrees.
        let level = platform_level_outline(0, vec![
            [0.0, -5.0, 0.0], [0.0, 5.0, 0.0], [3.0, 9.0, 0.0],
            [10.0, 9.0, 0.0], [10.0, -5.0, 0.0], [0.0, -5.0, 0.0],
        ]);
        let ledge = level.ledge(Vec3::new(-3.0, 0.0, 1.07), Vec2::X, 5.66).unwrap();
        assert!((ledge.left - 5.0).abs() < 1e-3 && (ledge.right - 5.0).abs() < 1e-3, "{ledge:?}");
        assert!((ledge.left_corner.unwrap() - Vec2::new(-0.8, 0.6)).length() < 1e-3, "{ledge:?}");
        let wall = crate::climb::find(&level, Vec3::new(-3.0, 0.0, 0.0), Vec2::X, 2.0, 5.4, 3.0).unwrap();
        assert!((wall.width - 14.0).abs() < 1e-3 && (wall.base_width - 10.0).abs() < 1e-3);
        assert_eq!(wall.left_extension, 4.0);
    }

    #[test]
    fn ledge_fan_keeps_the_full_span_across_split_straight_edges() {
        let mut level = platform_level(0);
        let mesh = Arc::get_mut(level.nav.as_mut().unwrap()).unwrap();
        for y in [-3.0, -1.0, 2.0, 4.0] {
            mesh.insert_vertex([0.0, y, 0.0]).unwrap();
        }
        let ledge = level.ledge(Vec3::new(-3.0, 0.0, 1.07), Vec2::X, 5.66).unwrap();
        assert!((ledge.left - 5.0).abs() < 1e-3 && (ledge.right - 5.0).abs() < 1e-3, "{ledge:?}");
        let wall = crate::climb::Wall::from_ledge(&ledge, 2.0);
        assert!((wall.width - 10.0).abs() < 1e-3, "90-degree corners must not extend: {wall:?}");
    }

    /// Walking away from the platform on open ground finds no wall.
    #[test]
    fn a_walk_away_from_the_wall_finds_none() {
        assert!(platform_level(0).ledge(Vec3::new(-3.0, 1.0, 1.07), -Vec2::X, 5.66).is_none());
    }
}
