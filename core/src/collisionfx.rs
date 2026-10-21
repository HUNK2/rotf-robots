//! CollisionTrigger history and CollisionFX presets (007dab90,007da7b0,007d9e40).
use std::collections::BTreeMap;
use glam::{Mat3,Quat,Vec3};
use crate::{formats::lxb::Node,particles::Template,vehicle::Contact};

#[derive(Clone,Debug)]
pub struct Def {
    pub mask: u32, pub types: u32, pub min_speed: f32, pub max_speed: f32,
    pub align: bool,
    /// soft/hard, each impact/roll/scrape. [data]
    pub particles: [[Option<Template>;3];2],
    pub sounds: [[u32;3];2],
}
impl Def {
    pub fn read(node:Node)->Self {
        let int=|name|node.get(name).and_then(Node::int).unwrap_or(0) as u32;
        let num=|name,default|node.get(name).and_then(Node::float).unwrap_or(default);
        let kinds=["impact","roll","scrape"];
        Self {mask:int("maskFilter"),types:node.get("typeFilter").and_then(Node::int).unwrap_or(0xffff) as u32,
            min_speed:num("minVelocity",0.0),max_speed:num("maxVelocity",1e37),align:int("alignToNormal")!=0,
            particles:std::array::from_fn(|hard|std::array::from_fn(|kind| {
                let info=node.path(&[if hard==0 {"softPFX"} else {"hardPFX"},kinds[kind]])?;
                if info.get("disable").and_then(Node::int).unwrap_or(0)!=0 {return None;}
                info.get("id").and_then(Template::read)
            })),
            sounds:std::array::from_fn(|hard|std::array::from_fn(|kind| {
                let Some(info)=node.path(&[if hard==0 {"softSFX"} else {"hardSFX"},kinds[kind]]) else {return 0;};
                if info.get("disable").and_then(Node::int).unwrap_or(0)!=0 {return 0;}
                info.get("override").and_then(Node::int).unwrap_or(0) as u32
            })),
        }
    }
    pub fn accepts(&self,contact:&Contact)->bool {
        self.accepts_speed(contact.velocity.length())
    }
    pub fn accepts_speed(&self,speed:f32)->bool {
        self.mask&1!=0 && self.types&1!=0 && speed>=self.min_speed && speed<=self.max_speed
    }
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind {Impact,Roll,Scrape,End}
impl Kind {pub fn index(self)->Option<usize> {match self {Self::Impact=>Some(0),Self::Roll=>Some(1),Self::Scrape=>Some(2),Self::End=>None}}}

/// SoundPhysicsReactor / CollisionSFX (01184450,0077f620).
#[derive(Clone, Copy, Debug, Default)]
pub struct SoundChoice { pub id: u32, pub once: bool, pub attached: bool }
#[derive(Clone, Debug)]
pub struct SoundDef {
    pub filter: Def,
    /// Hard, soft, bottom; each impact, roll, scrape. [data]
    pub choices: [[SoundChoice; 3]; 3],
}
impl SoundDef {
    pub fn read(node: Node) -> Self {
        Self { filter: Def::read(node), choices: ["hard", "soft", "bottom"].map(|material|
            ["impact", "roll", "scrape"].map(|kind| {
                let Some(info) = node.path(&[material, kind]) else { return SoundChoice::default() };
                if info.get("disable").and_then(Node::int) == Some(1) { return SoundChoice::default() }
                SoundChoice { id: info.get("override").and_then(Node::int).unwrap_or(0) as u32,
                    once: info.get("oneShot").and_then(Node::int) == Some(1),
                    attached: info.get("attachToObject").and_then(Node::int) == Some(1) }
            })) }
    }
    /// [game: 0077f620] Soft and bottom override only when they name a sound.
    /// `oneShot` consumes a preset entry, rather than changing the FEV's loop mode.
    pub fn select(&mut self, kind: Kind, hard: bool, bottom: bool) -> Option<SoundChoice> {
        let kind = kind.index()?;
        let material = if !hard && self.choices[1][kind].id != 0 { 1 } else { 0 };
        let selected = self.choices[if bottom && self.choices[2][kind].id != 0 { 2 } else { material }][kind];
        for row in [if hard { 0 } else { 1 }].into_iter().chain(bottom.then_some(2)) {
            if self.choices[row][kind].once { self.choices[row][kind].id = 0; }
        }
        (selected.id != 0).then_some(selected)
    }
}
#[derive(Clone,Copy,Debug)]
pub struct Event {pub key:u32,pub kind:Kind,pub point:Vec3,pub normal:Vec3,pub surface:u32,pub speed:f32,
    /// The sound's `impactForce`: max(n . (v_b - v_a), f) [game 0077f620 l.68-77, raw 0077fc00..0077fc89:
    /// v_a the holder at the record's +0, v_b at +8, n at +1c, the floor f at +5c]. Here B is the static
    /// world (v_b = 0), so it is the approach speed along the normal, at least 0. The holder order does
    /// not change it: swapping the holders and negating the normal leaves n . (v_b - v_a) the same, so
    /// the port's normal (out of the surface, toward the car) is all that is needed.
    /// The floor is NOT an impulse [game 006d7c20 l.312-327, 407/428]: the contact refresh copies the
    /// velocity of the holder at its param_1+0x1c0 (body +0xa0..+0xa8) into the record at +0x54..+0x5c, so
    /// f is that body's world velocity z, when it has a body. [assumed] the port takes f as 0.
    /// settles: the static-world case: with no body (006d7c20 l.312-318 zero branch writes the record at
    /// +0x6c, not +0x5c) the frame's +5c is not written in that call, so what it holds is the previous
    /// contact's copy; read whether the 006d7c20 frame is zeroed on entry, or a recording of impactForce
    /// for a car hit on the world.
    pub impact:f32}

/// `Event::impact` for a contact: the approach speed along its normal, never below 0.
fn impact_of(c:&Contact)->f32 {(-c.normal.dot(c.velocity)).max(0.0)}

/// The Lux physics type that the contact listener receives (record +0x3c), for a moving body with
/// `velocity` against a static surface whose `normal` points out of it. [game 006d7c20, contact_refresh,
/// the first kept slot]: approach = (v_b - v_a) . n = -n . v; 3 (initial push) when approach > .5 (00b38830);
/// else 1 (roll) when |sum (1 - |n_i|) d_i| > .7 (00b39c58), with d = v_a - v_b = v; else 5 when
/// approach <= .2 (00b34768), 2 (scrape) when not. The game takes it from every refresh, so a
/// repeated contact can be 3 again (a hard approach) and a first contact can be 1, 2 or 5.
/// [assumed] the port's `normal` points out of the surface, toward the car (as `impact_of` takes
/// it): the game's slot normal runs B toward A (notes/physics.md 1.6), and with the wrong sign every
/// approach would read negative. settles: the contact code's normal sign in vehicle.rs against a
/// head-on recording (the sparks' side), or a recording of the CollisionTrigger state for a hard hit.
pub fn physics_type(normal:Vec3,velocity:Vec3)->u32 {
    let approach=-normal.dot(velocity);
    if approach>0.5 {return 3;}
    let w=Vec3::ONE-normal.abs();
    if (w.x*velocity.x+w.y*velocity.y+w.z*velocity.z).abs()>0.7 {return 1;}
    if approach<=0.2 {5} else {2}
}

/// [game] 0056e7b0: +z along normal, +y from projected reference +y; use +x
/// as reference when +y is too nearly parallel. This fixes emitter twist too.
pub fn normal_rotation(normal:Vec3)->Quat {
    let z=normal.normalize_or_zero();
    if z==Vec3::ZERO {return Quat::IDENTITY;}
    let y=if z.dot(Vec3::Y).abs()<=0.96 {
        (Vec3::Y-z*z.y).normalize()
    } else {z.cross(Vec3::X).normalize()};
    Quat::from_mat3(&Mat3::from_cols(y.cross(z),y,z))
}
struct Track {contact:Contact,left:f32,history:[f32;10],sum:f32,kind:Option<Kind>,fresh:bool}
#[derive(Default)]
pub struct History {tracks:BTreeMap<u32,Track>,slot:usize}
impl History {
    /// Lux physics types: 1 roll, 2/5 sliding, 3/4 initial push, from `physics_type` above. [game 007dab90]
    /// A new entry takes 3/4 as its fresh impact (+0x28 = 1, life .25). On an entry that exists, 3/4 changes
    /// nothing (no point, no expiry, no history); 1 writes +1 and 2/5 write -1 at this update's slot, the
    /// point is taken and the life is .25 again.
    pub fn touch(&mut self,contact:Contact,physics_type:u32) {
        let t=self.tracks.entry(contact.key).or_insert_with(||Track {contact,left:0.25,
            history:[0.0;10],sum:0.0,kind:None,fresh:physics_type==3||physics_type==4});
        if physics_type==3 || physics_type==4 {return;}
        t.contact=contact;
        t.left=0.25;
        t.history[self.slot]=if physics_type==1 {1.0} else {-1.0};
    }
    pub fn contains(&self,key:u32)->bool {self.tracks.contains_key(&key)}
    /// Called once per original .032 s update. Keeps a ten-update signed history,
    /// threshold seven, and expires after .25 s without further contacts. [game 007da7b0]
    pub fn step(&mut self,dt:f32)->Vec<Event> {
        let old=self.slot; self.slot=(self.slot+1)%10;
        let mut events=Vec::new();
        self.tracks.retain(|&key,t| {
            let mut emit=|kind|events.push(Event {key,kind,point:t.contact.point,normal:t.contact.normal,
                surface:t.contact.surface,speed:t.contact.velocity.length(),impact:impact_of(&t.contact)});
            if t.left<=0.0 {emit(Kind::End);return false;}
            t.sum+=t.history[old]-t.history[self.slot];t.history[self.slot]=0.0;
            let kind=if t.sum>=7.0 && t.kind!=Some(Kind::Roll) {Some(Kind::Roll)}
                else if t.sum<=-7.0 && t.kind!=Some(Kind::Scrape) {Some(Kind::Scrape)}
                else if t.fresh && t.left==0.25 {Some(Kind::Impact)} else {None};
            if let Some(kind)=kind {emit(kind);t.kind=Some(kind);}
            t.fresh=false; t.left-=dt;
            true
        }); events
    }
    pub fn clear(&mut self)->Vec<Event> {
        let events=self.tracks.iter().map(|(&key,t)|Event {key,kind:Kind::End,
            point:t.contact.point,normal:t.contact.normal,surface:t.contact.surface,
            speed:t.contact.velocity.length(),impact:impact_of(&t.contact)}).collect();
        self.tracks.clear();events
    }
    pub fn point(&self,key:u32)->Option<Vec3> {self.tracks.get(&key).map(|t|t.contact.point)}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn contact()->Contact {Contact {key:7,fraction:0.0,point:Vec3::ZERO,normal:Vec3::X,depth:0.0,surface:0,velocity:Vec3::Y*20.0}}
    #[test]
    fn collision_sound_overrides_and_once_flags_follow_the_material_branch() {
        let filter = Def { mask: 1, types: 1, min_speed: 0.0, max_speed: 100.0, align: false,
            particles: std::array::from_fn(|_| std::array::from_fn(|_| None)), sounds: [[0; 3]; 2] };
        let mut sounds = SoundDef { filter, choices: [[SoundChoice::default(); 3]; 3] };
        sounds.choices[0][0] = SoundChoice { id: 1, once: true, attached: true };
        sounds.choices[2][0] = SoundChoice { id: 2, once: true, attached: false };
        // Soft's empty entry falls back to hard, but consumes only soft's once flag.
        assert_eq!(sounds.select(Kind::Impact, false, false).unwrap().id, 1);
        assert_eq!(sounds.choices[0][0].id, 1);
        assert_eq!(sounds.select(Kind::Impact, true, true).unwrap().id, 2);
        assert_eq!(sounds.choices[0][0].id, 0);
        assert_eq!(sounds.choices[2][0].id, 0);
        assert!(sounds.select(Kind::Impact, true, true).is_none());
        assert!(sounds.select(Kind::End, true, true).is_none());
    }
    #[test]
    fn contact_alignment_keeps_reference_twist_and_handles_parallel_normal() {
        for z in [Vec3::Z,Vec3::X,Vec3::Y,Vec3::NEG_Y,Vec3::new(0.2,0.4,0.8).normalize()] {
            let q=normal_rotation(z);
            assert!((q*Vec3::Z).abs_diff_eq(z,1e-6));
            assert!((q*Vec3::X).cross(q*Vec3::Y).abs_diff_eq(z,1e-6));
        }
        assert_eq!(normal_rotation(Vec3::Z),Quat::IDENTITY);
    }
    #[test]
    fn physics_type_follows_contact_refresh_speeds() {
        // A falling body (approach 2): an initial push, whatever the contact was before.
        assert_eq!(physics_type(Vec3::Z,Vec3::NEG_Z*2.0),3);
        // Sliding fast along a floor (approach 0.1, tangential 10): roll.
        assert_eq!(physics_type(Vec3::Z,Vec3::new(10.0,0.0,-0.1)),1);
        // Slow slide (approach 0.1, tangential 0.5): slide 5; approach 0.4: scrape 2.
        assert_eq!(physics_type(Vec3::Z,Vec3::new(0.5,0.0,-0.1)),5);
        assert_eq!(physics_type(Vec3::Z,Vec3::new(0.5,0.0,-0.4)),2);
        // Approach just above .5 wins over the tangential test.
        assert_eq!(physics_type(Vec3::Z,Vec3::new(10.0,0.0,-0.6)),3);
    }
    #[test]
    fn initial_impact_scrape_window_and_end_are_separate_events() {
        let mut h=History::default();h.touch(contact(),3);
        assert_eq!(h.step(0.032)[0].kind,Kind::Impact);
        for _ in 0..6 {h.touch(contact(),5);assert!(h.step(0.032).is_empty());}
        h.touch(contact(),5);assert_eq!(h.step(0.032)[0].kind,Kind::Scrape);
        h.touch(contact(),5);assert!(h.step(0.032).is_empty());
        let mut end=0;
        for _ in 0..10 {end+=h.step(0.032).iter().filter(|e|e.kind==Kind::End).count();}
        assert_eq!(end,1);assert!(!h.contains(7));
    }
    #[test]
    fn sparse_rolling_contacts_do_not_cross_the_ten_update_threshold() {
        let mut h=History::default();
        for i in 0..60 {if i%2==0 {h.touch(contact(),1);}
            assert!(h.step(0.032).iter().all(|e|e.kind!=Kind::Roll));}
        for _ in 0..10 {h.touch(contact(),1);h.step(0.032);}
        assert_eq!(h.tracks[&7].kind,Some(Kind::Roll));
    }
}
