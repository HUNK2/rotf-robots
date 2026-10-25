//! The particle effects of every playable pack, counted the way `effects_dump` lists them for
//! Bumblebee: clip effect events, footstep and melee presets, effects on the robot and vehicle
//! objects, and the weapons' muzzle / trail / blast / impact effects -- each with how many of
//! its templates hold something the port does not read (`Template::unread`) and which renderer
//! kinds they use.
//! `cargo run -p tf2-core --example roster_effects` (reads the install).

use std::collections::BTreeMap;
use std::path::Path;

use tf2_core::formats::anim::EffectEventKind;
use tf2_core::particles::Template;

const PACKS: [&str; 13] = [
    "bumblebee", "optimus", "ratchet", "ironhide", "megatron", "longhaul", "audir8", "jetfire", "firestorm",
    "starscream", "blackout", "optimusflight", "megatronflight",
];

#[derive(Default)]
struct Tally {
    templates: usize,
    unread: usize,
    unread_what: BTreeMap<String, usize>,
    renderers: BTreeMap<String, usize>,
}

impl Tally {
    fn add(&mut self, t: &Template) {
        self.templates += 1;
        let missing = t.unread();
        if !missing.is_empty() {
            self.unread += 1;
        }
        for m in missing {
            *self.unread_what.entry(m).or_default() += 1;
        }
        for e in &t.elements {
            *self.renderers.entry(String::from_utf8_lossy(&e.renderer.id.to_be_bytes()).into_owned()).or_default() += 1;
        }
    }
}

fn main() {
    let dir = tf2_core::install::dir_string();
    let mut renderers_all: BTreeMap<String, usize> = BTreeMap::new();
    println!("{:<15} {:>5} {:>5} {:>5} {:>5} {:>5} {:>6} {:>6}", "pack", "clip", "step", "melee", "obj", "weap", "tmpl", "unread");
    for pack in PACKS {
        let data = match tf2_core::character::load(Path::new(&dir), pack) {
            Ok(d) => d,
            Err(e) => {
                println!("{pack:<15} LOAD FAILED: {e:?}");
                continue;
            }
        };
        let mut tally = Tally::default();
        let mut clip = 0;
        for refs in data.animations.sets.values() {
            for r in refs {
                let Some(c) = &r.clip else { continue };
                for event in &c.effect_events {
                    if let EffectEventKind::Spawn { template, .. } = &event.kind {
                        clip += 1;
                        tally.add(template);
                    }
                }
            }
        }
        let count = |infos: &mut dyn Iterator<Item = Option<&Template>>, tally: &mut Tally| {
            let mut n = 0;
            for t in infos.flatten() {
                n += 1;
                tally.add(t);
            }
            n
        };
        let step = count(&mut data.step_impact.infos.iter().map(|i| i.template.as_ref()), &mut tally);
        let melee = count(&mut data.melee_impact.infos.iter().map(|i| i.template.as_ref()), &mut tally);
        let obj = count(
            &mut [Some(&data.robot), data.vehicle.as_ref()].into_iter().flatten().flat_map(|o| &o.effects).map(|d| Some(&d.template)),
            &mut tally,
        );
        let weap = count(
            &mut data.weapon_effects.iter().flat_map(|w| {
                [w.muzzle_flash.as_ref(), w.trail.as_ref(), w.blast.as_ref()].into_iter().chain(w.impact.infos.iter().map(|i| i.template.as_ref()))
            }),
            &mut tally,
        );
        println!("{pack:<15} {clip:>5} {step:>5} {melee:>5} {obj:>5} {weap:>5} {:>6} {:>6}", tally.templates, tally.unread);
        for w in &data.weapon_effects {
            println!(
                "    weapon {:08x}: muzzle {} trail {} blast {} impacts {} blast ssd {} flight sounds {} blast sounds {}",
                w.name,
                w.muzzle_flash.is_some(),
                w.trail.is_some(),
                w.blast.is_some(),
                w.impact.infos.len(),
                w.blast_ssd.len(),
                w.flight_sounds.len(),
                w.blast_sounds.len()
            );
        }
        for (what, n) in &tally.unread_what {
            println!("    unread {what} x{n}");
        }
        for (r, n) in tally.renderers {
            *renderers_all.entry(r).or_default() += n;
        }
    }
    println!("renderer kinds over the roster: {renderers_all:?}");
}
