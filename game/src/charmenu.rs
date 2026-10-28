//! Test aid, not the game: the robot menu. F2 opens it; Up / Down (or W / S) choose, Enter
//! swaps to that robot where he stands, F2 or Backspace closes. Unlike F6 / F7 (`switcher.rs`,
//! which start the game again), the swap happens in the running game: the new robot's pack,
//! model, animation sets, weapons and sound banks are read on a thread of their own while the
//! old one plays on, then his entities and resources are replaced in one frame (`apply`).
//!
//! `load` and `arm` are also how `main` loads the first character, so the two cannot drift.
//!
//! Limits (test aid, no tags): he comes back on foot, out of any form, at full state, facing
//! as before; the arena's enemies stay the ones loaded at the start (a robot on their side
//! cannot hurt them: the menu says so, and F6 starts the game again with the other scouts);
//! with `--no-sound` the new robot is silent too.
//!
//! Capture aid: `--swap-at <s>,<pack>` swaps once at that time (keys are not read under `--shot`).

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use tf2_core::character::CharacterData;
use tf2_core::formats::characterlist::Spec as CharacterSpec;
use tf2_core::sim;
use tf2_core::tuning::{self, Tuning};

use crate::animation::AnimLibrary;
use crate::player::{Controls, NoVehicle, Player, TuningRes};
use crate::weapons::{Arsenal, AxeModel, DebrisModel, GrenadeModel, MissileModel, TurretModel, WeaponModel};

/// A character as `main` and the swap need him: his tuning (with the numbers measured on his
/// clips and model), his data and his clips.
pub struct Loaded {
    pub tuning: Tuning,
    pub data: CharacterData,
    pub animations: AnimLibrary,
}

/// Reads a character from the install: tuning, pack and clips, and fills in what is measured
/// on them. `args` gives the camera distance levels (`--robot-camera`, `--drive-camera`).
pub fn load(game_dir: &Path, spec: &CharacterSpec, texture_scale: u32, args: &[String]) -> Result<Loaded, String> {
    let mut tuning = tuning::load(game_dir, &spec.attributes).map_err(|e| e.to_string())?;
    crate::startup_mark("tuning read");
    let mut data = tf2_core::character::load(game_dir, &spec.pack).map_err(|e| e.to_string())?;
    crate::startup_mark("character pack read");
    // Grindor's cannon: what his cinematic wave borrows (`tf2_core::energon_wave`) [port design],
    // read beside the rest (0.6 s: Devastator's pack and a search of his own). `--original-wave`
    // keeps the original's wave alone, to compare.
    let cinematic = !args.iter().any(|a| a == "--original-wave");
    let energon = (cinematic && data.weapons.iter().any(|w| w.name == tf2_core::energon_wave::CANNON)).then(|| {
        let (dir, pack) = (game_dir.to_path_buf(), spec.pack.clone());
        std::thread::spawn(move || tf2_core::energon_wave::read(&dir, &pack))
    });
    crate::texture_upscale::apply(&mut data.library, texture_scale);
    crate::startup_mark("textures ready");
    // The character's +0x170 is its pack's `nameID` (00713300 l.192-199). [game]
    if data.name_id != 0 {
        tuning.name_hash = data.name_id;
    }
    // Devastator (`tf2_core::giant::is_giant`): his clips' root moved onto his chest before
    // anything is measured on them (`tf2_core::giant::reroot`).
    let giant = tf2_core::giant::is_giant(&data);
    if giant {
        tf2_core::giant::reroot(&mut data.animations, &data.robot);
    }
    let animations =
        AnimLibrary { robot: std::mem::take(&mut data.animations), vehicle: std::mem::take(&mut data.vehicle_animations) };
    tuning.locomotion = tuning::Locomotion::from_animations(&animations.robot, &mut tuning.missing);
    tuning.actions =
        tuning::Actions::from_animations(&animations.robot, &data.robot, &mut tuning.missing)
            .with_vehicle(data.vehicle.as_ref())
            .with_melee(&data.melee_weapons);
    tuning.weapon_set = tuning::RuleSet::from_animations(&animations.robot, "WeaponSet", &mut tuning.missing);
    tuning.rule_sets = tuning::RuleSet::all(&animations.robot);
    for name in &tuning.missing {
        eprintln!("tuning value not found in the game data: {name}");
    }
    for problem in &data.library.problems {
        eprintln!("skipped: {problem}");
    }
    // The axle spacing is not a tuning number; measure it on the vehicle model.
    if let Some(vehicle) = &data.vehicle {
        let along = |name: &str| vehicle.nodes.iter().find(|n| n.name == name).map(|n| n.bind[3][1]);
        if let (Some(front), Some(rear)) = (along("Tire_Front_L"), along("Tire_Rear_L")) {
            tuning.wheelbase = (front - rear).abs();
        }
    }
    // The game has three distance levels for each camera. The defaults are the ones the
    // recorded game ran with: the near follow camera and the far driving camera.
    let level = |flag: &str, default: usize| crate::flag_value(args, flag).and_then(|v| v.parse::<f32>().ok()).map_or(default, |v| v as usize);
    if let Some(camera) = tuning.robot_camera_levels.get(level("--robot-camera", 0)) {
        tuning.robot_camera = camera.clone();
    }
    if let Some(camera) = tuning.drive_camera_levels.get(level("--drive-camera", 2)) {
        tuning.drive_camera = camera.clone();
    }
    // Devastator: the walking giant's player binding, sets, root and camera (`tf2_core::giant`).
    // Not every object with a `ShockWave` (the two flight packs have one each, `is_giant`).
    if giant {
        // The three reads of his pack below run beside the fitting (each opens the pack: 0.7 s one
        // after another; performance audit, 2026-10-23).
        let (vortex_def, pods, dressing) = std::thread::scope(|s| {
            let vortex = s.spawn(|| tf2_core::giant::read_vortex(game_dir));
            let pods = s.spawn(|| tf2_core::giant::read_pods(game_dir));
            let dressing = s.spawn(|| tf2_core::giant::read_dressing(game_dir));
            tf2_core::giant::fit(&mut tuning, &animations.robot, &data.robot, &data.shockwaves);
            (vortex.join().expect("reading his vortex panicked"), pods.join().expect("reading his pods panicked"),
                dressing.join().expect("reading his dressing panicked"))
        });
        // The port's planting of his legs and arms (`tf2_core::giant::limbs`) [assumed].
        if let Some(giant) = tuning.giant.as_mut() {
            giant.limbs = tf2_core::giant::limbs(&animations.robot, &data.robot, &data.library);
            // His vortex: the mission's funnel put on his head (`tf2_core::giant::vortex`) [assumed].
            giant.vortex = vortex_def
                .and_then(|def| tf2_core::giant::vortex(&def, &data.library, &animations.robot, &data.robot));
            if giant.vortex.is_none() {
                eprintln!("devastator: no vortex (the boss mission's VortexObj was not read)");
            }
            // His guns: the mission's pods on his attach points (`tf2_core::giant::read_pods`) [assumed].
            giant.pods = pods;
            data.weapon_effects.extend(giant.pods.iter().flat_map(|p| p.effects.iter().cloned()));
            // The bank's sounds the port gives him (`tf2_core::giant::SOUNDS`) [assumed port design].
            data.extra_sounds.extend(tf2_core::giant::SOUNDS);
            // His dressing: pod smoke, sparks, bursts and the burning plume (`read_dressing`) [assumed port design].
            giant.dressing = dressing;
            eprintln!("devastator: {} pod weapons ({} pods)", giant.pods.len(), giant.pods.iter().map(|p| p.mounts.len()).sum::<usize>());
        }
    }
    crate::startup_mark("animations measured");
    // His wave's dressing: Devastator's material and textures join his library (his own ids
    // first), the sounds of Devastator's bank it plays are loaded with his.
    match energon.map(|reading| reading.join().map_err(|_| "reading it panicked".to_owned()).and_then(|r| r)) {
        Some(Ok((dressing, mut borrowed))) => {
            crate::texture_upscale::apply(&mut borrowed, texture_scale);
            for (id, m) in borrowed.materials {
                data.library.materials.entry(id).or_insert(m);
            }
            for (id, t) in borrowed.textures {
                data.library.textures.entry(id).or_insert(t);
            }
            for (id, f) in borrowed.texture_frames {
                data.library.texture_frames.entry(id).or_insert(f);
            }
            data.extra_sounds.extend(tf2_core::energon_wave::SOUNDS);
            data.energon = Some(dressing);
            crate::startup_mark("energon wave dressed");
        }
        Some(Err(e)) => eprintln!("energon wave: not dressed ({e})"),
        None => {}
    }
    Ok(Loaded { tuning, data, animations })
}

/// His weapons: the lists his pack names, his special's from his attributes, and what each
/// round's end shows and starts.
pub fn arm(game_dir: &Path, attributes: &str, data: &CharacterData, giant: Option<&tf2_core::giant::Giant>) -> Option<Arsenal> {
    // Devastator's pack lists no weapons: his are the boss mission's pods (`Arsenal::load_giant`).
    if let Some(giant) = giant.filter(|g| !g.pods.is_empty()) {
        return Arsenal::load_giant(game_dir, &giant.pods);
    }
    Arsenal::load(game_dir, &data.weapons).map(|mut arsenal| {
        // His special's weapons, from his attributes (`characterAttributes<X>`): none for
        // Bumblebee, whose weapon name is 0. [data]
        if let Err(e) = arsenal.equip_special(game_dir, attributes, &data.weapons, &data.weapon_effects) {
            eprintln!("special: no weapon ({e})");
        }
        // What each round's end shows and starts (its blast rumble, its ground wave). [data]
        arsenal.set_effects(&data.weapon_effects);
        // Grindor's cinematic wave's ring (`tf2_core::energon_wave`) [port design].
        arsenal.set_energon(data.energon.as_ref().map(|d| d.ring.clone()));
        arsenal
    })
}

/// Everything a swap puts in place, made on the loading thread.
struct Ready {
    loaded: Loaded,
    arsenal: Option<Arsenal>,
    sound: Option<crate::sound::SoundOut>,
    heal_areas: Vec<crate::targets::HealArea>,
    rates: tf2_core::repair::Rates,
    flash_lights: Vec<(u32, tf2_core::point_light::FlashLight)>,
}

struct Loading {
    choice: usize,
    handle: JoinHandle<Result<Ready, String>>,
    began: f32,
}

#[derive(Resource)]
pub struct Menu {
    game_dir: PathBuf,
    texture_scale: u32,
    args: Vec<String>,
    /// The roster (`switcher::ROSTER`): the name shown and the character list's spec.
    choices: Vec<(&'static str, CharacterSpec)>,
    /// The arena's enemy, for the sound banks the new robot's output also needs.
    enemy: CharacterSpec,
    sound: bool,
    current: Option<usize>,
    open: bool,
    cursor: usize,
    loading: Option<Loading>,
    message: String,
    /// `--swap-at <s>,<pack>`, until it has been done.
    scripted: Option<(f32, usize)>,
    /// The message stays up until then after a swap with the menu closed.
    show_until: f32,
}

impl Menu {
    pub fn new(game_dir: &Path, texture_scale: u32, args: &[String], roster: &tf2_core::formats::characterlist::CharacterList,
               enemy: &CharacterSpec, current: Option<usize>) -> Self {
        let choices = crate::switcher::ROSTER.iter().map(|&(pack, name)| {
            // `spec_by_pack` as `--character` does: it knows Devastator, who has no `characters\` pack.
            (name, roster.find(pack).and_then(|e| roster.spec(e)).unwrap_or_else(|| roster.spec_by_pack(pack)))
        }).collect();
        let scripted = crate::flag_value(args, "--swap-at").and_then(|v| {
            let (at, pack) = v.split_once(',')?;
            Some((at.trim().parse().ok()?, crate::switcher::index_of(Some(pack.trim()))?))
        });
        if crate::flag_value(args, "--swap-at").is_some() && scripted.is_none() {
            eprintln!("--swap-at wants <seconds>,<pack of the roster>: ignored");
        }
        Self {
            game_dir: game_dir.to_owned(),
            texture_scale,
            args: args.to_vec(),
            choices,
            enemy: enemy.clone(),
            sound: !args.iter().any(|a| a == "--no-sound"),
            current,
            open: false,
            cursor: current.unwrap_or(0),
            loading: None,
            message: String::new(),
            scripted,
            show_until: 0.0,
        }
    }

    /// Starts reading robot `choice` on a thread of his own.
    fn begin(&mut self, choice: usize, now: f32, scout: std::sync::Arc<tf2_core::combat::Scout>) {
        let (name, spec) = self.choices[choice].clone();
        let (game_dir, scale, args, enemy, sound) = (self.game_dir.clone(), self.texture_scale, self.args.clone(), self.enemy.clone(), self.sound);
        eprintln!("robot menu: loading {name} (pack {}, attributes {})", spec.pack, spec.attributes);
        self.message = format!("loading {name}...");
        let handle = std::thread::spawn(move || {
            let loaded = load(&game_dir, &spec, scale, &args)?;
            let arsenal = arm(&game_dir, &spec.attributes, &loaded.data, loaded.tuning.giant.as_ref());
            let sound = if sound { crate::sound::start(&game_dir, &spec, &enemy, &loaded.data, &loaded.animations, &loaded.tuning, &scout) } else { None };
            let heal_areas = crate::heal_areas(&game_dir.join("characters").join(format!("{}.str", spec.pack)));
            let rates = crate::read_rates(&game_dir, &spec.attributes);
            let flash_lights = tf2_core::point_light::load_flash_lights(&game_dir, &spec.pack).unwrap_or_default();
            Ok(Ready { loaded, arsenal, sound, heal_areas, rates, flash_lights })
        });
        self.loading = Some(Loading { choice, handle, began: now });
    }
}

#[derive(Component)]
pub struct MenuText;

pub fn spawn(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(16.0),
            top: Val::Px(170.0),
            padding: UiRect::all(Val::Px(10.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
        TextFont::from_font_size(16.0),
        TextColor(Color::srgb(0.95, 0.95, 0.9)),
        GlobalZIndex(50),
        Visibility::Hidden,
        MenuText,
    ));
}

/// The menu's keys, and the player's input held while it is open (the arrows and Enter are
/// its, not his).
pub fn run(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    shot: Option<Res<crate::Shot>>,
    encounter: Res<crate::targets::Encounter>,
    mut menu: ResMut<Menu>,
    mut controls: ResMut<Controls>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let now = time.elapsed_secs();
    if let Some((at, choice)) = menu.scripted {
        if now >= at && menu.loading.is_none() {
            menu.scripted = None;
            menu.begin(choice, now, encounter.scout.clone());
        }
        return;
    }
    if shot.is_some() {
        return;
    }
    if keys.just_pressed(KeyCode::F2) {
        menu.open = !menu.open;
    }
    if !menu.open {
        return;
    }
    // The mouse is free while it is open, as Esc leaves it.
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
    let count = menu.choices.len();
    if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) {
        menu.cursor = (menu.cursor + 1) % count;
    }
    if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) {
        menu.cursor = (menu.cursor + count - 1) % count;
    }
    if keys.just_pressed(KeyCode::Backspace) {
        menu.open = false;
    }
    if (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter)) && menu.loading.is_none() {
        let choice = menu.cursor;
        if Some(choice) == menu.current {
            menu.message = format!("already {}", menu.choices[choice].0);
        } else {
            menu.begin(choice, now, encounter.scout.clone());
        }
    }
    let c = &mut *controls;
    (c.stick, c.look, c.vehicle, c.brake) = (Vec2::ZERO, Vec2::ZERO, 0.0, 0.0);
    (c.aim, c.fire, c.switch, c.recentre_pressed) = (false, false, false, false);
    (c.jump_held, c.jump_pressed, c.melee_held, c.melee_pressed) = (false, false, false, false);
    (c.climb_held, c.climb_pressed, c.special_held, c.special_pressed) = (false, false, false, false);
}

pub fn draw(time: Res<Time>, menu: Res<Menu>, text: Single<(&mut Text, &mut Visibility), With<MenuText>>) {
    let (mut text, mut visibility) = text.into_inner();
    let loading = menu.loading.as_ref();
    // Shown while open, while a swap is being read, and for a moment after one (so a scripted
    // one is seen in captures).
    let show = menu.open || loading.is_some() || time.elapsed_secs() < menu.show_until;
    *visibility = if show { Visibility::Visible } else { Visibility::Hidden };
    if !show {
        return;
    }
    let mut out = String::from("ROBOTS  (F2 close, Up/Down choose, Enter swap)\n\n");
    for (i, (name, _)) in menu.choices.iter().enumerate() {
        let mark = if i == menu.cursor { ">" } else { " " };
        let note = if Some(i) == menu.current {
            "  (playing)"
        } else if loading.is_some_and(|l| l.choice == i) {
            "  (loading)"
        } else {
            ""
        };
        out.push_str(&format!(" {mark} {name}{note}\n"));
    }
    if let Some(l) = loading {
        out.push_str(&format!("\nloading {}... {:.1} s", menu.choices[l.choice].0, time.elapsed_secs() - l.began));
    } else if !menu.message.is_empty() {
        out.push_str(&format!("\n{}", menu.message));
    }
    if **text != out {
        **text = out;
    }
}

/// Puts the robot read on the loading thread in place of the one playing, once he is read.
/// Exclusive: everything of the old one goes and the new one is made in the same frame, before
/// anything reads input or moves.
pub fn apply(world: &mut World) {
    let finished = world.resource::<Menu>().loading.as_ref().is_some_and(|l| l.handle.is_finished());
    if !finished {
        return;
    }
    let Some(loading) = world.resource_mut::<Menu>().loading.take() else { return };
    let choice = loading.choice;
    let name = world.resource::<Menu>().choices[choice].0;
    let ready = match loading.handle.join() {
        Ok(Ok(ready)) => ready,
        Ok(Err(e)) => {
            eprintln!("robot menu: {name} did not load ({e})");
            world.resource_mut::<Menu>().message = format!("{name} did not load: {e}");
            return;
        }
        Err(_) => {
            eprintln!("robot menu: loading {name} failed (the thread panicked)");
            world.resource_mut::<Menu>().message = format!("{name} did not load (see the console)");
            return;
        }
    };
    // The weapon systems skip while there is no arsenal (`main`), so a robot with no weapons
    // (Devastator) takes it away and the next one puts his back.
    if ready.arsenal.is_none() {
        eprintln!("robot menu: {name} has no arsenal; the weapon systems rest");
    }
    let Some(old) = world.query::<&Player>().iter(world).next().map(|p| p.state.clone()) else { return };
    let Ready { mut loaded, arsenal, sound, heal_areas, rates, flash_lights } = ready;

    // He takes over where the old one stands, as he faces; his own body otherwise.
    let tuning = &mut loaded.tuning;
    let old_rig_levels = world.resource::<crate::camera::Rig>().levels;
    if let Some(s) = tuning.robot_camera_levels.get(old_rig_levels[0]) { tuning.robot_camera = s.clone(); }
    if let Some(s) = tuning.weapon_camera_levels.get(old_rig_levels[1]) { tuning.weapon_camera = s.clone(); }
    if let Some(s) = tuning.drive_camera_levels.get(old_rig_levels[2]) { tuning.drive_camera = s.clone(); }
    let mut start = sim::State::new(tuning);
    start.pos = old.pos;
    start.yaw = old.yaw;
    start.turbo_upgrades = old.turbo_upgrades;
    start.difficulty = old.difficulty;
    start.flight_boundary = old.flight_boundary;
    let mut rig = crate::camera::Rig::new(&start, tuning, None, None);
    {
        let old_rig = world.resource::<crate::camera::Rig>();
        rig.camera.options = old_rig.camera.options;
        rig.orbit = old_rig.orbit;
        rig.zoom = old_rig.zoom;
    }
    let team = tuning.damage.team;

    // Everything of the old robot goes: his entity (robot and car under it), his weapons'
    // models and pools, the stand-in car, his effects' quads and his ribbons.
    let mut gone: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<Player>, With<NoVehicle>, With<WeaponModel>, With<GrenadeModel>, With<TurretModel>, With<MissileModel>, With<AxeModel>, With<DebrisModel>)>>()
        .iter(world)
        .collect();
    gone.extend(world.resource_mut::<crate::effects::Effects>().reset());
    gone.extend(world.resource_mut::<crate::trails::Trails>().reset());
    for entity in gone {
        if let Ok(entity) = world.get_entity_mut(entity) {
            entity.despawn();
        }
    }
    if let Some(old_sound) = world.remove_resource::<crate::sound::SoundOut>() {
        old_sound.shut_down();
    }
    if let Some(sound) = sound {
        world.insert_resource(sound);
    }
    match arsenal {
        Some(arsenal) => world.insert_resource(arsenal),
        None => drop(world.remove_resource::<Arsenal>()),
    }
    world.insert_resource(TuningRes(loaded.tuning));
    world.insert_resource(crate::GameData(loaded.data));
    world.insert_resource(loaded.animations);
    world.insert_resource(crate::effects::FlashLights(flash_lights));
    world.insert_resource(crate::Start(start));
    world.insert_resource(rig);
    world.insert_resource(crate::weapons::PoolCache::default());
    world.insert_resource(crate::weapons::PoolDemand::default());
    world.insert_resource(crate::weapons::DebrisDemand::default());
    world.insert_resource(crate::ssd::Marks::default());
    let enemy_team = {
        let mut encounter = world.resource_mut::<crate::targets::Encounter>();
        encounter.heal.areas = heal_areas;
        encounter.heal.user = rates;
        encounter.scout.tuning.damage.team
    };
    if let Err(e) = world.run_system_once(crate::spawn_player) {
        eprintln!("robot menu: spawning {name} failed ({e})");
    }
    if world.contains_resource::<Arsenal>()
        && let Err(e) = world.run_system_once(crate::weapons::attach_models) {
        eprintln!("robot menu: hanging {name}'s weapons failed ({e})");
    }
    let _ = world.run_system_once(crate::hud::refresh_ability_icon);

    let now = world.resource::<Time>().elapsed_secs();
    let mut menu = world.resource_mut::<Menu>();
    menu.current = Some(choice);
    menu.cursor = choice;
    menu.show_until = now + 3.0;
    menu.message = if team == enemy_team {
        format!("now {name}; the arena's scouts are on his side (F6 restarts with the other scouts)")
    } else {
        format!("now {name}")
    };
    eprintln!("robot menu: now {name} (at {now:.2} s, read in {:.2} s)", now - loading.began);
}
