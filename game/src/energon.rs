//! Grindor's cinematic energon wave, dressed (`tf2_core::energon_wave`) [port design]: what the
//! ring arc (`weapons::EnergonWave`, drawn by `shockwaves`) adds round it, every piece the game's
//! own. At the impact: his ground slam's blast and his heavy landing's dust blown up
//! (`Template::scaled`), the wave object's Light as a wide flash, and `EXP_HUGE` with
//! Devastator's pound and the game's massive shake arriving as late as the camera is far (sound
//! at 340 m/s). Along the front: his landing dust and a building's sparks as it passes, and
//! Devastator's vortex rolling with its middle, fading as the ring ends.

use bevy::math::Affine3A;
use bevy::prelude::*;
use tf2_core::energon_wave::{self as ew, Front, Kick};
use tf2_core::sound::Handle;

use crate::assets::bevy_to_game;

/// [port design] How long before the ring ends its roll starts to fade, in seconds.
const ROLL_FADE: f32 = 0.5;

/// What a wave sets off later, once the boom reaches the camera.
enum Due {
    Sound(u32),
    Shake,
}

/// The waves being dressed, and what is on its way to the camera.
#[derive(Default)]
pub struct Dressed {
    waves: Vec<Live>,
    due: Vec<(f32, Due, Vec3)>,
}

struct Live {
    number: u32,
    front: Front,
    roll: Option<Handle>,
}

#[allow(clippy::too_many_arguments)]
pub fn dress(
    time: Res<Time>,
    data: Res<crate::GameData>,
    tuning: Res<crate::player::TuningRes>,
    arsenal: Option<Res<crate::weapons::Arsenal>>,
    sound: Option<Res<crate::sound::SoundOut>>,
    mut effects: ResMut<crate::effects::Effects>,
    mut rig: ResMut<crate::camera::Rig>,
    camera: Single<&Transform, (With<Camera3d>, Without<crate::ocean::OceanReflection>, Without<crate::duel::SecondView>)>,
    mut seen: Local<Dressed>,
) {
    let Some(dressing) = data.0.energon.as_ref() else { return };
    let dt = time.delta_secs();
    let seen = &mut *seen;
    let eye = bevy_to_game(camera.translation);
    let waves = arsenal.as_ref().map_or(&[][..], |a| a.energon_waves());
    let placed = |scale: f32, turn: Quat, at: Vec3| Affine3A::from_scale_rotation_translation(Vec3::splat(scale), turn, at);

    // What the boom brings as it reaches the camera.
    seen.due.retain_mut(|(left, due, at)| {
        *left -= dt;
        if *left > 0.0 {
            return true;
        }
        match due {
            Due::Sound(id) => {
                if let Some(s) = &sound {
                    s.play_event_at(*id, *at);
                }
            }
            Due::Shake => rig.start_named_shake_at(ew::IMPACT_SHAKE, &tuning.0, *at),
        }
        false
    });

    for wave in waves {
        let Some(ring) = wave.ring.rings.first() else { continue };
        if !seen.waves.iter().any(|l| l.number == wave.number) {
            // The impact: the blast, flash and boom where the missile ended, the dust on the
            // ground the ring opens on (the same place, but for a shot into a wall: at its foot).
            let at = wave.impact;
            if let Some(blast) = &dressing.impact {
                effects.start_energon(&blast.scaled(ew::IMPACT_BLAST_SCALE), placed(ew::IMPACT_BLAST_SCALE, Quat::IDENTITY, at + Vec3::Z));
            }
            if let Some(dust) = &dressing.dust {
                effects.start_energon(&dust.scaled(ew::IMPACT_DUST_SCALE), placed(ew::IMPACT_DUST_SCALE, Quat::IDENTITY, ring.centre));
            }
            let row = data.0.weapon_effects.iter().find(|w| w.name == ew::CANNON).and_then(|w| w.wave.as_ref());
            // The energy wall: the wave object's own emitter, laid along the front (`front_frame`).
            if let (Some(emitter), Some(arsenal)) = (row.and_then(|w| w.emitter.as_ref()), &arsenal) {
                let mut sized = emitter.clone();
                sized.template = emitter.template.scaled(ew::FRONT_SIZE);
                for slot in 0..ew::FRONT_SLOTS {
                    if let Some(frame) = arsenal.energon_frame(wave.number, slot) {
                        effects.start_energon_front(&sized, wave.number, slot, frame);
                    }
                }
            }
            // The flash: the wave object's own Light, wider, brighter and short.
            if let Some(light) = row.and_then(|w| w.light.as_ref()) {
                let mut flash = light.clone();
                flash.light.range *= ew::FLASH_RANGE;
                flash.light.colour *= ew::FLASH_COLOUR;
                (flash.time, flash.fade_start) = (ew::FLASH_TIME, ew::FLASH_FADE_START);
                effects.flash(&flash, Affine3A::from_translation(at + Vec3::Z * 2.0));
            }
            let late = (at - eye).length() / ew::SOUND_SPEED;
            seen.due.extend([(late, Due::Sound(ew::IMPACT_SOUND), at), (late, Due::Sound(ew::POUND_SOUND), at), (late, Due::Shake, at)]);
            let roll = sound.as_ref().and_then(|s| s.start_kept_at(ew::ROLL_SOUND, None, at));
            seen.waves.push(Live { number: wave.number, front: Front::default(), roll });
        }
        let Some(live) = seen.waves.iter_mut().find(|l| l.number == wave.number) else { continue };
        for kick in live.front.step(&wave.ring, &wave.ground, dt) {
            match kick {
                Kick::Dust { at, scale } => {
                    if let Some(dust) = &dressing.dust {
                        effects.start_energon(&dust.scaled(scale), placed(scale, Quat::IDENTITY, at));
                    }
                }
                // The building sparks spray along their +x: out from the impact and up.
                Kick::Sparks { at, way } => {
                    if let Some(sparks) = &dressing.sparks {
                        let turn = Quat::from_rotation_arc(Vec3::X, (way + Vec3::Z * 0.6).normalize());
                        effects.start_energon(sparks, placed(1.0, turn, at + Vec3::Z * 0.5));
                    }
                }
            }
        }
        // The roll rides the front's middle (held at a wall) and fades as the wave runs out.
        if let (Some(s), Some(handle), Some(arc)) = (&sound, live.roll, ring.arc) {
            s.move_to(handle, wave.ground.point(arc.angle(), wave.ring.band(ring.radius).1));
            let left = (wave.ground.end_radius(&wave.ring) - ring.radius) / wave.ring.def.speed.max(f32::EPSILON);
            s.set_volume(handle, (left / ROLL_FADE).clamp(0.0, 1.0));
        }
    }
    seen.waves.retain(|live| {
        let alive = waves.iter().any(|w| w.number == live.number);
        if !alive && let (Some(s), Some(handle)) = (&sound, live.roll) {
            s.stop(handle);
        }
        alive
    });
}
