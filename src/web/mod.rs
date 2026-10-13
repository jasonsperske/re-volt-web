//! Browser entry point. JavaScript owns the page (file picking, DOM events,
//! the animation loop) and calls into [`App`]; Rust owns loading, simulation,
//! rendering and input interpretation, including Gamepad API polling.

mod gl;
mod input;
mod shop;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use glam::{Mat4, Quat, Vec3};
use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{future_to_promise, JsFuture};
use web_sys::HtmlCanvasElement;

use crate::ai::AiDriver;
use crate::assets::{Entry, GameData};
use crate::car::{collide_cars, Car, CarSetup, DriveInput};
use crate::collision::{level_world, CollisionWorld};
use crate::disc::ByteSource;
use crate::formats::fan::AiNodes;
use crate::formats::inf::LevelInfo;
use crate::race::{LapEvent, LapTracker, Track};
use crate::scene::{self, gl_model, to_gl};
use crate::weapons::{Weapon, Weapons};
use crate::camera::{Camera, CameraMode};

/// Byte reads served by a JS `(offset, length) => Promise<Uint8Array>`,
/// normally backed by `File.slice()` so the image is never fully loaded.
pub(crate) struct JsSource(Function);

impl ByteSource for JsSource {
    async fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        let promise = self
            .0
            .call2(&JsValue::NULL, &JsValue::from_f64(offset as f64), &JsValue::from_f64(len as f64))
            .map_err(js_err)?;
        let value = JsFuture::from(Promise::from(promise)).await.map_err(js_err)?;
        Ok(Uint8Array::new(&value).to_vec())
    }
}

fn js_err(v: JsValue) -> String {
    v.as_string().unwrap_or_else(|| format!("{v:?}"))
}

struct LoadedLevel {
    id: String,
    info: LevelInfo,
    gpu: gl::GpuScene,
    world: CollisionWorld,
    /// `None` for arenas, which have no laps.
    track: Option<Track>,
    ai_nodes: AiNodes,
    objects: Vec<crate::formats::fob::LevelObject>,
}

const COUNTDOWN: f32 = 3.0;
/// Cars that never race as opponents (hidden or novelty cars).
const NOT_OPPONENTS: [&str; 7] = ["TROLLEY", "UFO", "Q", "WINCAR", "WINCAR2", "WINCAR3", "WINCAR4"];

struct Race {
    /// Seconds until GO (cars are held while positive).
    countdown: f32,
    time: f32,
    laps: u32,
}

struct CarPart {
    gpu: gl::GpuScene,
    kind: crate::assets::PartKind,
    offset: Vec3,
}

/// Where to draw each copy of a car part this frame (the aerial's section
/// model is drawn once per section).
fn car_part_models(sim: &Car, part: &CarPart) -> Vec<Mat4> {
    sim.render_part_transforms(part.kind, part.offset)
        .into_iter()
        .map(|(pos, rot, scale)| gl_model(pos, rot) * Mat4::from_scale(scale))
        .collect()
}

/// A car model loaded once and shared by every racer driving it.
struct LoadedCar {
    name: String,
    /// Engine sound file (game path).
    engine_sound: &'static str,
    parts: Vec<CarPart>,
    setup: CarSetup,
}

struct Racer {
    model: Rc<LoadedCar>,
    sim: Car,
    /// `None` for the player.
    ai: Option<AiDriver>,
    tracker: Option<LapTracker>,
    finished: Option<f32>,
}

struct State {
    renderer: gl::Renderer,
    frontend: Option<shop::Shop>,
    /// Draw the front end instead of the race.
    show_frontend: bool,
    /// The chosen car standing on the shop floor (game car index).
    shop_car: Option<(i32, Car, Rc<LoadedCar>)>,
    input: input::Input,
    camera: Camera,
    level: Option<LoadedLevel>,
    models: HashMap<String, Rc<LoadedCar>>,
    player_car: Option<String>,
    opponent_cars: Vec<String>,
    /// Index 0 is the player.
    racers: Vec<Racer>,
    /// Weapon and pickup models by file stem (e.g. "PICKUP"), shared.
    fx_models: HashMap<&'static str, gl::GpuScene>,
    weapons: Option<Weapons>,
    pickups_enabled: bool,
    /// Weapon sounds since JS last collected them: (path, volume 0..1).
    pending_sounds: Vec<(&'static str, f32)>,
    /// Set by the on-screen Flip button; handled on the next frame.
    flip_requested: bool,
    /// Workbench changes per car id, applied whenever the car is built.
    tunings: HashMap<String, crate::tuning::Tuning>,
    /// Whether the shop car is standing on the workbench.
    on_bench: bool,
    race: Option<Race>,
    laps: u32,
    /// Menus are open: nothing moves and player input is ignored.
    paused: bool,
    last_time: Option<f64>,
    gamepad: Option<String>,
}

impl State {
    fn start_pose(&self) -> Option<(Vec3, Quat)> {
        let info = &self.level.as_ref()?.info;
        Some((Vec3::from(info.start_pos), Quat::from_rotation_y(info.start_rot * std::f32::consts::TAU)))
    }

    fn player(&self) -> Option<&Racer> {
        self.racers.first()
    }

    /// Lines everyone up on the grid and restarts the countdown. The level's
    /// start position is pole; the player starts at the back.
    fn spawn(&mut self) {
        let (Some(level), Some((pos, rot))) = (&self.level, self.start_pose()) else { return };
        let Some(player) = self.player_car.as_ref().and_then(|id| self.models.get(id)).cloned() else { return };
        let mut field: Vec<(Rc<LoadedCar>, bool)> =
            self.opponent_cars.iter().filter_map(|id| self.models.get(id)).map(|m| (m.clone(), false)).collect();
        field.push((player, true));
        let forward = rot * Vec3::Z;
        let right = rot * Vec3::X;
        let mut racers = Vec::new();
        for (slot, (model, is_player)) in field.into_iter().enumerate() {
            let row = (slot / 2) as f32;
            let side = if slot % 2 == 0 { -1.0 } else { 1.0 };
            let at = pos - forward * (row * 200.0) + right * (side * if self.opponent_cars.is_empty() { 0.0 } else { 70.0 });
            let mut sim = Car::new(model.setup.clone(), at, rot);
            sim.place_on_ground(&level.world, at, rot);
            let tracker = level.track.as_ref().map(|t| LapTracker::new(t, sim.pos));
            let ai = (!is_player).then(|| AiDriver::new(&level.ai_nodes, sim.pos, slot as u32 + 1));
            let racer = Racer { model, sim, ai, tracker, finished: None };
            if is_player {
                racers.insert(0, racer);
            } else {
                racers.push(racer);
            }
        }
        self.racers = racers;
        // Pickup spawns come from the level's .fob; switching pickups off
        // skips them, as the original's loader does.
        let objects: Vec<_> = if self.pickups_enabled { level.objects.clone() } else { Vec::new() };
        let seed = (js_sys::Math::random() * 1e9) as u32;
        self.weapons = Some(Weapons::new(&objects, self.racers.len(), seed));
        self.race = Some(Race { countdown: COUNTDOWN, time: 0.0, laps: self.laps });
        self.snap_camera();
    }

    /// Jumps the camera straight behind the player.
    fn snap_camera(&mut self) {
        let Some(sim) = self.player().map(|r| &r.sim) else { return };
        let (pos, rot) = sim.render_pose();
        self.camera.snap_behind(to_gl(pos), to_gl(rot * Vec3::Z));
    }

    /// Racer indices in race order: finishers by time, then by distance.
    fn standings(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.racers.len()).collect();
        let key = |r: &Racer| -> (f32, f32) {
            match r.finished {
                Some(t) => (0.0, t),
                None => (1.0, -r.tracker.as_ref().map_or(0.0, |t| t.progress())),
            }
        };
        order.sort_by(|&a, &b| {
            let (ka, kb) = (key(&self.racers[a]), key(&self.racers[b]));
            ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
        });
        order
    }
}

/// Models for pickups and weapons, in MODELS/. FIREWORK's textured faces use
/// page 0, mapped to the first effects sheet (GFX/FXPAGE1.BMP).
const FX_MODELS: [&str; 7] = ["PICKUP", "FIREWORK", "WBOMB", "FUSE", "BALL", "STAR", "SPHERE"];

/// Loads the pickup/weapon models once per disc.
async fn ensure_fx_models(game: &GameData<JsSource>, state: &Rc<RefCell<State>>) -> Result<(), JsValue> {
    if !state.borrow().fx_models.is_empty() {
        return Ok(());
    }
    let mut textures = BTreeMap::new();
    if let Ok(img) = game.read("GFX/FXPAGE1.BMP").await.and_then(|d| crate::formats::bmp::decode(&d)) {
        textures.insert(0i16, img);
    }
    for name in FX_MODELS {
        let Ok(mesh) = game.load_mesh(&format!("MODELS/{name}.M")).await else { continue };
        let mut builder = scene::SceneBuilder::default();
        builder.add_mesh(&mesh, &scene::Placement::at([0.0; 3]), |t| Some(t));
        let gpu = state.borrow().renderer.upload(&builder, &textures)?;
        state.borrow_mut().fx_models.insert(name, gpu);
    }
    // Oil slicks: a dark translucent unit disc (no model on the disc; the
    // original draws its own quad).
    let mut builder = scene::SceneBuilder::default();
    builder.add_mesh(&oil_disc(), &scene::Placement::at([0.0; 3]), |_| None);
    let gpu = state.borrow().renderer.upload(&builder, &BTreeMap::new())?;
    state.borrow_mut().fx_models.insert("OIL", gpu);
    Ok(())
}

fn oil_disc() -> crate::formats::prm::Mesh {
    use crate::formats::prm::{Mesh, Poly, Vertex, POLY_DOUBLE_SIDED, POLY_SEMI_TRANSPARENT};
    const SIDES: usize = 20;
    let mut verts = vec![Vertex { pos: [0.0; 3], normal: [0.0, -1.0, 0.0] }];
    for k in 0..SIDES {
        let a = k as f32 / SIDES as f32 * std::f32::consts::TAU;
        verts.push(Vertex { pos: [a.cos(), 0.0, a.sin()], normal: [0.0, -1.0, 0.0] });
    }
    let color = 0xd0_10_12_18u32;
    let polys = (0..SIDES)
        .map(|k| Poly {
            flags: POLY_DOUBLE_SIDED | POLY_SEMI_TRANSPARENT,
            texture: -1,
            indices: [0, 1 + k as u16, 1 + ((k + 1) % SIDES) as u16, 0],
            colors: [color; 4],
            uvs: [[0.0; 2]; 4],
        })
        .collect();
    Mesh { polys, verts }
}

/// Loads and uploads a car model unless it's already cached.
async fn ensure_model(game: &GameData<JsSource>, state: &Rc<RefCell<State>>, id: &str) -> Result<(), JsValue> {
    if state.borrow().models.contains_key(id) {
        return Ok(());
    }
    let mut car = game.load_car(id).await?;
    if let Some(t) = state.borrow().tunings.get(id) {
        t.apply(&mut car.params);
    }
    let (lo, hi) = car.body_bounds();
    let setup = CarSetup::new(&car.params, car.hull.as_ref(), lo, hi);
    let textures: BTreeMap<i16, _> = car.texture.as_ref().map(|t| (0, t.clone())).into_iter().collect();
    let mut st = state.borrow_mut();
    let mut parts = Vec::new();
    for p in scene::car_parts(&car) {
        let gpu = st.renderer.upload(&p.builder, &textures)?;
        parts.push(CarPart { gpu, kind: p.kind, offset: Vec3::from(p.offset) });
    }
    // Engine loops by car type. Inferred from file names, not from code.
    let engine_sound = match (car.id.as_str(), car.params.class) {
        ("UFO", _) => "WAVS/UFO.WAV",
        (id, _) if id.starts_with("WINCAR") => "WAVS/CLOCKWRK.WAV",
        (_, 1) => "WAVS/PETROL.WAV",
        _ => "WAVS/MOTO.WAV",
    };
    st.models.insert(id.to_string(), Rc::new(LoadedCar { name: car.params.name, engine_sound, parts, setup }));
    Ok(())
}

#[wasm_bindgen]
pub struct App {
    state: Rc<RefCell<State>>,
    game: Rc<RefCell<Option<Rc<GameData<JsSource>>>>>,
    car_ids: Rc<RefCell<Vec<String>>>,
    level_generation: Rc<Cell<u32>>,
    car_generation: Rc<Cell<u32>>,
}

#[wasm_bindgen]
impl App {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement) -> Result<App, JsValue> {
        console_error_panic_hook::set_once();
        let state = State {
            renderer: gl::Renderer::new(canvas)?,
            frontend: None,
            show_frontend: false,
            shop_car: None,
            input: input::Input::default(),
            camera: Camera::new(Vec3::new(0.0, 500.0, 1500.0), 0.0, -0.3),
            level: None,
            models: HashMap::new(),
            player_car: None,
            opponent_cars: Vec::new(),
            racers: Vec::new(),
            fx_models: HashMap::new(),
            weapons: None,
            pickups_enabled: true,
            pending_sounds: Vec::new(),
            flip_requested: false,
            tunings: HashMap::new(),
            on_bench: false,
            race: None,
            laps: 3,
            paused: false,
            last_time: None,
            gamepad: None,
        };
        Ok(App {
            state: Rc::new(RefCell::new(state)),
            game: Rc::new(RefCell::new(None)),
            car_ids: Rc::new(RefCell::new(Vec::new())),
            level_generation: Rc::new(Cell::new(0)),
            car_generation: Rc::new(Cell::new(0)),
        })
    }

    /// Opens a disc image through `reader(offset, length) -> Promise<Uint8Array>`.
    /// Resolves to `{ levels: [{id, name}], cars: [{id, name}] }`.
    pub fn open_disc(&self, reader: Function) -> Promise {
        let game_slot = self.game.clone();
        let car_ids = self.car_ids.clone();
        future_to_promise(async move {
            let game = Rc::new(GameData::open(JsSource(reader)).await?);
            let levels = game.list_levels().await?;
            let cars = game.list_cars().await?;
            *car_ids.borrow_mut() = cars.iter().map(|c| c.id.clone()).collect();
            *game_slot.borrow_mut() = Some(game);
            let result = Object::new();
            Reflect::set(&result, &"levels".into(), &entries(&levels))?;
            Reflect::set(&result, &"cars".into(), &entries(&cars))?;
            Ok(result.into())
        })
    }

    pub fn load_level(&self, id: String) -> Promise {
        let (game, state) = (self.game.borrow().clone(), self.state.clone());
        let generation = self.level_generation.clone();
        let mine = generation.get().wrapping_add(1);
        generation.set(mine);
        future_to_promise(async move {
            let game = game.ok_or("no disc open")?;
            ensure_fx_models(&game, &state).await?;
            let level = game.load_level(&id).await?;
            if generation.get() != mine {
                return Ok(JsValue::FALSE); // superseded by a newer selection
            }
            let built = scene::level_scene(&level);
            let world = level_world(&level);
            let track = Track::new(level.zones.clone(), level.pos_nodes.clone());
            let mut st = state.borrow_mut();
            let gpu = st.renderer.upload(&built, &level.textures)?;
            if let Some(old) = st.level.take() {
                st.renderer.destroy(old.gpu);
            }
            st.level = Some(LoadedLevel { id: level.id, info: level.info, gpu, world, track, ai_nodes: level.ai_nodes, objects: level.objects });
            st.spawn();
            Ok(JsValue::TRUE)
        })
    }

    pub fn load_car(&self, id: String) -> Promise {
        let (game, state) = (self.game.borrow().clone(), self.state.clone());
        let generation = self.car_generation.clone();
        let mine = generation.get().wrapping_add(1);
        generation.set(mine);
        future_to_promise(async move {
            let game = game.ok_or("no disc open")?;
            ensure_model(&game, &state, &id).await?;
            if generation.get() != mine {
                return Ok(JsValue::FALSE);
            }
            let mut st = state.borrow_mut();
            st.player_car = Some(id);
            st.spawn();
            Ok(JsValue::TRUE)
        })
    }

    /// Picks `count` random opponents (never the player's car or novelty
    /// cars), loads them and restarts the race.
    pub fn set_opponents(&self, count: u32) -> Promise {
        let (game, state, car_ids) = (self.game.borrow().clone(), self.state.clone(), self.car_ids.clone());
        future_to_promise(async move {
            let game = game.ok_or("no disc open")?;
            let player = state.borrow().player_car.clone();
            let mut pool: Vec<String> = car_ids
                .borrow()
                .iter()
                .filter(|id| !NOT_OPPONENTS.contains(&id.as_str()) && Some(*id) != player.as_ref())
                .cloned()
                .collect();
            let mut chosen = Vec::new();
            while chosen.len() < count.min(7) as usize && !pool.is_empty() {
                let i = (js_sys::Math::random() * pool.len() as f64) as usize % pool.len();
                chosen.push(pool.swap_remove(i));
            }
            for id in &chosen {
                ensure_model(&game, &state, id).await?;
            }
            let mut st = state.borrow_mut();
            st.opponent_cars = chosen;
            st.spawn();
            Ok(JsValue::TRUE)
        })
    }

    /// Flips the player's car back onto its wheels (the on-screen button;
    /// F and gamepad LB do the same).
    pub fn flip_car(&self) {
        self.state.borrow_mut().flip_requested = true;
    }

    /// Restarts the race from the grid.
    pub fn reset_car(&self) {
        self.state.borrow_mut().spawn();
    }

    /// Switches between the chase camera and the free-fly camera.
    pub fn toggle_camera(&self) {
        let mut st = self.state.borrow_mut();
        st.camera.mode = match st.camera.mode {
            CameraMode::Chase => CameraMode::Free,
            CameraMode::Free => CameraMode::Chase,
        };
    }

    pub fn key(&self, code: &str, down: bool) {
        self.state.borrow_mut().input.key(code, down);
    }

    pub fn mouse_move(&self, dx: f32, dy: f32) {
        self.state.borrow_mut().input.mouse_move(dx, dy);
    }

    /// Drops held keys, e.g. when the page loses focus.
    pub fn release_keys(&self) {
        self.state.borrow_mut().input.clear();
    }

    /// Advances and renders one frame; `time_ms` is the rAF timestamp.
    pub fn frame(&self, time_ms: f64) {
        let mut guard = self.state.borrow_mut();
        let st = &mut *guard;
        let dt = st.last_time.map(|t| ((time_ms - t) / 1000.0) as f32).unwrap_or(0.0).clamp(0.0, 0.1);
        st.last_time = Some(time_ms);

        let controls = st.input.take(dt);
        st.gamepad = controls.gamepad.clone();
        if st.show_frontend {
            if let Some(shop) = &mut st.frontend {
                shop.update(dt);
                // A chosen box has landed: drop the real car out of it.
                if let Some((car, pos, rot)) = shop.drop_request.take() {
                    let id = crate::frontend::CAR_ORDER.get(car as usize).copied().unwrap_or("");
                    let model = st.player_car.as_ref().filter(|p| p.as_str() == id || car > 27).and_then(|p| st.models.get(p)).cloned();
                    if let Some(model) = model {
                        let flat = Quat::from_rotation_y((rot * Vec3::Z).x.atan2((rot * Vec3::Z).z) + std::f32::consts::PI);
                        let mut sim = Car::new(model.setup.clone(), pos, flat);
                        sim.place(pos, flat);
                        st.shop_car = Some((car, sim, model));
                    }
                }
                if let Some((_, sim, _)) = &mut st.shop_car {
                    sim.update(&shop.world, DriveInput::default(), dt);
                }
            }
            self.render(st);
            return;
        }
        if st.paused {
            self.render(st);
            return;
        }
        if controls.toggle_camera {
            st.camera.mode = if st.camera.mode == CameraMode::Chase { CameraMode::Free } else { CameraMode::Chase };
        }
        if controls.reset_car {
            st.spawn();
        }
        let held = st.race.as_ref().is_some_and(|r| r.countdown > 0.0);
        if std::mem::take(&mut st.flip_requested) | controls.flip_car && !held {
            if let Some(player) = st.racers.iter_mut().find(|r| r.ai.is_none()) {
                player.sim.flip();
            }
        }

        // Simulate every racer. Nobody moves during the countdown; the player
        // only drives while the chase camera is active.
        let player_input = match st.camera.mode {
            CameraMode::Chase if !held => DriveInput { throttle: controls.throttle, steer: controls.steer },
            _ => DriveInput::default(),
        };
        if let Some(level) = &st.level {
            let racing = st.race.as_ref().is_some_and(|r| r.countdown <= 0.0);
            let race_time = st.race.as_ref().map_or(0.0, |r| r.time);
            let laps = st.race.as_ref().map_or(u32::MAX, |r| r.laps);
            for racer in &mut st.racers {
                let input = match &mut racer.ai {
                    Some(ai) if !held => ai.drive(&level.ai_nodes, &level.world, &mut racer.sim, dt),
                    Some(_) => DriveInput::default(),
                    None => player_input,
                };
                racer.sim.update(&level.world, input, dt);
                if racing && racer.finished.is_none() {
                    if let (Some(tracker), Some(track)) = (&mut racer.tracker, &level.track) {
                        if let Some(LapEvent::Lap { n, .. }) = tracker.update(track, racer.sim.pos, dt) {
                            if n >= laps {
                                racer.finished = Some(race_time);
                            }
                        }
                    }
                }
                // Fell out of the world: back to the start.
                if racer.sim.pos.y > level.info.start_pos[1] + 20000.0 {
                    let (pos, rot) = (Vec3::from(level.info.start_pos), Quat::from_rotation_y(level.info.start_rot * std::f32::consts::TAU));
                    racer.sim.place_on_ground(&level.world, pos, rot);
                }
            }
            for i in 0..st.racers.len() {
                let (head, tail) = st.racers.split_at_mut(i + 1);
                for other in tail {
                    collide_cars(&mut head[i].sim, &mut other.sim);
                }
            }
            if !held {
                let order = st.standings();
                let mut positions = vec![0; st.racers.len()];
                for (place, &i) in order.iter().enumerate() {
                    positions[i] = place;
                }
                let cpu: Vec<bool> = st.racers.iter().map(|r| r.ai.is_some()).collect();
                let mut fire = vec![false; st.racers.len()];
                if let Some(f) = fire.first_mut() {
                    *f = controls.fire && st.camera.mode == CameraMode::Chase;
                }
                if let Some(weapons) = &mut st.weapons {
                    let mut cars: Vec<&mut Car> = st.racers.iter_mut().map(|r| &mut r.sim).collect();
                    weapons.update(&level.world, &mut cars, &positions, &cpu, &fire, dt);
                    let ear = st.racers.first().map_or(Vec3::ZERO, |r| r.sim.pos);
                    for s in &weapons.sounds {
                        let volume = (1.0 - s.pos.distance(ear) / 6000.0).clamp(0.0, 1.0);
                        if volume > 0.02 {
                            st.pending_sounds.push((s.path, volume));
                        }
                    }
                }
            }
            if let Some(race) = &mut st.race {
                if race.countdown > 0.0 {
                    race.countdown -= dt;
                } else {
                    race.time += dt;
                }
            }
        }

        match (st.camera.mode, st.racers.first().map(|r| &r.sim), &st.level) {
            (CameraMode::Chase, Some(sim), Some(level)) => {
                let world = &level.world;
                // Keep the camera on the near side of walls between it and the car.
                let limit = |from: Vec3, to: Vec3| {
                    let (a, b) = (to_gl(from), to_gl(to)); // to_gl is its own inverse
                    let d = b - a;
                    match world.raycast(a, d.normalize_or_zero(), d.length(), crate::collision::NCP_CAMERA_IGNORES) {
                        Some(hit) => to_gl(a + d.normalize_or_zero() * (hit.t - 10.0).max(20.0)),
                        None => to,
                    }
                };
                let (pos, rot) = sim.render_pose();
                st.camera.chase(to_gl(pos), to_gl(rot * Vec3::Z), controls.yaw, dt, limit);
            }
            _ => st.camera.fly(&controls.fly(), dt),
        }

        self.render(st);
    }
}

impl App {
    fn render(&self, st: &mut State) {
        if st.show_frontend {
            if let Some(shop) = &st.frontend {
                let c = shop.info.fog_color.map(|v| v as f32 / 255.0);
                let fog = gl::Fog { color: c, start: shop.info.fog_start, end: shop.info.far_clip };
                let aspect = st.renderer.resize();
                let proj = Mat4::perspective_rh_gl(55f32.to_radians(), aspect, 5.0, shop.info.far_clip.max(1000.0));
                st.renderer.begin(&fog);
                let mut scenes = shop.scenes();
                if let Some((_, sim, model)) = &st.shop_car {
                    for part in &model.parts {
                        scenes.extend(car_part_models(sim, part).into_iter().map(|m| (&part.gpu, m)));
                    }
                }
                st.renderer.draw(&scenes, proj * shop.view(), shop.eye(), &fog);
                return;
            }
        }
        let (fog, far) = match &st.level {
            Some(l) => {
                let c = l.info.fog_color.map(|v| v as f32 / 255.0);
                (gl::Fog { color: c, start: l.info.fog_start, end: l.info.far_clip }, l.info.far_clip)
            }
            None => (gl::Fog { color: [0.08, 0.09, 0.12], start: 1e9, end: 1e9 + 1.0 }, 50000.0),
        };
        let aspect = st.renderer.resize();
        let proj = Mat4::perspective_rh_gl(60f32.to_radians(), aspect, 5.0, far.max(1000.0));
        let view_proj = proj * st.camera.view();

        st.renderer.begin(&fog);
        let mut scenes = Vec::new();
        if let Some(l) = &st.level {
            scenes.push((&l.gpu, Mat4::IDENTITY));
        }
        for racer in &st.racers {
            for part in &racer.model.parts {
                scenes.extend(car_part_models(&racer.sim, part).into_iter().map(|m| (&part.gpu, m)));
            }
        }
        if let Some(weapons) = &st.weapons {
            for v in weapons.visuals() {
                if let Some(gpu) = st.fx_models.get(v.model) {
                    let model = gl_model(v.pos, v.rot) * Mat4::from_scale(Vec3::splat(v.scale));
                    scenes.push((gpu, model));
                }
            }
        }
        st.renderer.draw(&scenes, view_proj, st.camera.pos, &fog);
    }
}

#[wasm_bindgen]
impl App {
    /// Turns pickups on or off for the next race (restarts the current one).
    pub fn set_pickups(&self, on: bool) {
        let mut st = self.state.borrow_mut();
        st.pickups_enabled = on;
        st.spawn();
    }

    /// Pickups currently on the track (tests, debugging).
    pub fn live_pickups(&self) -> u32 {
        self.state.borrow().weapons.as_ref().map_or(0, |w| w.live_pickups() as u32)
    }

    /// Weapon sounds since the last call, as `[{path, volume}]`.
    pub fn take_sounds(&self) -> Array {
        let mut st = self.state.borrow_mut();
        st.pending_sounds
            .drain(..)
            .map(|(path, volume)| {
                let o = Object::new();
                let _ = Reflect::set(&o, &"path".into(), &path.into());
                let _ = Reflect::set(&o, &"volume".into(), &JsValue::from_f64(volume as f64));
                JsValue::from(o)
            })
            .collect()
    }

    /// Hands the player a weapon (debugging and tests): index as in
    /// docs/weapons.md §3.1 (0 shockwave … 10 global pulse).
    pub fn give_weapon(&self, index: usize) {
        let mut guard = self.state.borrow_mut();
        let st = &mut *guard;
        if let (Some(w), Some(weapons)) = (Weapon::ALL.get(index).copied(), &mut st.weapons) {
            let mut cars: Vec<&mut Car> = st.racers.iter_mut().map(|r| &mut r.sim).collect();
            weapons.grant(0, w, &mut cars);
        }
    }

    /// Loads the toy-shop front end (once).
    pub fn load_frontend(&self) -> Promise {
        let (game, state) = (self.game.borrow().clone(), self.state.clone());
        future_to_promise(async move {
            if state.borrow().frontend.is_some() {
                return Ok(JsValue::TRUE);
            }
            let game = game.ok_or("no disc open")?;
            let data = shop::ShopData::load(&game).await?;
            let mut st = state.borrow_mut();
            let shop = shop::Shop::new(data, &st.renderer)?;
            st.frontend = Some(shop);
            Ok(JsValue::TRUE)
        })
    }

    /// Shows the front end instead of the race (menus) or hides it.
    pub fn show_frontend(&self, on: bool) {
        let mut st = self.state.borrow_mut();
        st.show_frontend = on && st.frontend.is_some();
        st.input.clear();
    }

    pub fn frontend_shown(&self) -> bool {
        self.state.borrow().show_frontend
    }

    /// Moves the shop camera to node `node` over `seconds` (docs/frontend.md
    /// §1); 0 cuts.
    pub fn fe_goto(&self, node: usize, seconds: f32) {
        if let Some(shop) = &mut self.state.borrow_mut().frontend {
            if seconds <= 0.0 {
                shop.rig.cut(node);
            } else {
                shop.rig.go(node, seconds);
            }
        }
    }

    pub fn fe_camera_arrived(&self) -> bool {
        self.state.borrow().frontend.as_ref().is_none_or(|s| s.rig.arrived())
    }

    /// Stores workbench changes for a car (`null` for stock): an object
    /// with an optional `name` and multipliers keyed as in
    /// `tuning::KEYS`. A car that's already loaded is rebuilt, and the car
    /// on the workbench takes the new setup where it stands.
    pub fn set_car_tuning(&self, id: String, tuning: JsValue) -> Promise {
        let (game, state) = (self.game.borrow().clone(), self.state.clone());
        future_to_promise(async move {
            let mut t = crate::tuning::Tuning::default();
            if tuning.is_object() {
                t.name = Reflect::get(&tuning, &"name".into())?.as_string();
                for key in crate::tuning::KEYS {
                    if let Some(v) = Reflect::get(&tuning, &key.into())?.as_f64() {
                        t.set(key, v as f32);
                    }
                }
            }
            let loaded = {
                let mut st = state.borrow_mut();
                if t.is_stock() {
                    st.tunings.remove(&id);
                } else {
                    st.tunings.insert(id.clone(), t);
                }
                st.models.remove(&id).is_some()
            };
            if let (true, Some(game)) = (loaded, game) {
                ensure_model(&game, &state, &id).await?;
                let mut st = state.borrow_mut();
                let model = st.models.get(&id).cloned();
                let is_player = st.player_car.as_deref() == Some(id.as_str());
                if let (Some(model), true, Some((_, sim, shown))) = (model, is_player, &mut st.shop_car) {
                    sim.setup = model.setup.clone();
                    *shown = model;
                }
            }
            Ok(JsValue::TRUE)
        })
    }

    /// Puts the highlighted car on the workbench (the shop counter) and
    /// moves the camera there, or clears the bench and goes back to the
    /// car shelf.
    pub fn fe_workbench(&self, on: bool) {
        let mut guard = self.state.borrow_mut();
        let st = &mut *guard;
        st.on_bench = on;
        st.shop_car = None;
        let Some(shop) = &mut st.frontend else { return };
        if !on {
            shop.rig.go(crate::frontend::node::SELECT_CAR, crate::frontend::MOVE_TIME);
            return;
        }
        shop.rig.go(crate::frontend::node::WORKBENCH, crate::frontend::MOVE_TIME);
        let model = st.player_car.as_ref().and_then(|id| st.models.get(id)).cloned();
        if let (Some(model), Some(car)) = (model, shop.selected_car) {
            let rot = Quat::from_rotation_y(crate::frontend::BENCH_YAW);
            let mut sim = Car::new(model.setup.clone(), Vec3::from(crate::frontend::BENCH_POS), rot);
            sim.place_on_ground(&shop.world, Vec3::from(crate::frontend::BENCH_POS) - Vec3::Y * 40.0, rot);
            st.shop_car = Some((car, sim, model));
        }
    }

    /// Highlights a car on the shelf (its box slides out) and loads its
    /// model for when it's chosen; `null` puts every box back.
    pub fn fe_select_car(&self, id: Option<String>) -> Promise {
        let (game, state) = (self.game.borrow().clone(), self.state.clone());
        future_to_promise(async move {
            {
                let mut st = state.borrow_mut();
                let index = id.as_ref().map(|id| {
                    crate::frontend::CAR_ORDER.iter().position(|c| c.eq_ignore_ascii_case(id)).map_or(28, |i| i as i32)
                });
                if let Some(shop) = &mut st.frontend {
                    shop.selected_car = index;
                    shop.chosen = false;
                }
                st.shop_car = None;
            }
            if let (Some(game), Some(id)) = (game, id) {
                ensure_model(&game, &state, &id).await?;
                state.borrow_mut().player_car = Some(id);
            }
            Ok(JsValue::TRUE)
        })
    }

    /// Confirms (or un-confirms) the highlighted car: the box flies to the
    /// floor and the car drops out of it.
    pub fn fe_choose_car(&self, chosen: bool) {
        let mut st = self.state.borrow_mut();
        if let Some(shop) = &mut st.frontend {
            shop.chosen = chosen;
        }
        if !chosen {
            st.shop_car = None;
        }
    }

    /// Whether the chosen car has been dropped onto the floor.
    pub fn fe_car_shown(&self) -> bool {
        self.state.borrow().shop_car.is_some()
    }

    /// Shows a track's picture on the shop screens.
    pub fn fe_set_track(&self, id: String) -> Promise {
        let (game, state) = (self.game.borrow().clone(), self.state.clone());
        future_to_promise(async move {
            let game = game.ok_or("no disc open")?;
            let img = match game.read(&format!("GFX/{id}.BMP")).await {
                Ok(d) => crate::formats::bmp::decode(&d)?,
                Err(_) => crate::formats::bmp::decode(&game.read("GFX/ACCLAIM.BMP").await?)?,
            };
            // The screen shows the whole picture; black isn't see-through here.
            let mut img = img;
            for px in img.rgba.chunks_exact_mut(4) {
                px[3] = 255;
            }
            let mut st = state.borrow_mut();
            let st = &mut *st;
            if let Some(shop) = &mut st.frontend {
                shop.set_screen_image(&st.renderer, &img)?;
            }
            Ok(JsValue::TRUE)
        })
    }

    /// Turns the name wheel to letter `index` (0–29, docs/frontend.md §3.5).
    pub fn fe_set_wheel(&self, index: usize) {
        if let Some(shop) = &mut self.state.borrow_mut().frontend {
            shop.wheel.index = index % crate::frontend::WHEEL_SIZE;
        }
    }

    /// Decodes a BMP from the disc: `{width, height, rgba}` (black is
    /// transparent, as in the game).
    pub fn read_image(&self, path: String) -> Promise {
        let game = self.game.borrow().clone();
        future_to_promise(async move {
            let game = game.ok_or("no disc open")?;
            let img = crate::formats::bmp::decode(&game.read(&path).await?)?;
            let o = Object::new();
            Reflect::set(&o, &"width".into(), &JsValue::from(img.width))?;
            Reflect::set(&o, &"height".into(), &JsValue::from(img.height))?;
            Reflect::set(&o, &"rgba".into(), &js_sys::Uint8ClampedArray::from(img.rgba.as_slice()).into())?;
            Ok(o.into())
        })
    }

    /// The game's PC strings (`STRINGS/ENGLISH.TXT`), by id.
    pub fn strings(&self) -> Promise {
        let game = self.game.borrow().clone();
        future_to_promise(async move {
            let game = game.ok_or("no disc open")?;
            let list = crate::frontend::parse_strings(&game.read("STRINGS/ENGLISH.TXT").await?);
            Ok(list.iter().map(|s| JsValue::from(s.as_str())).collect::<Array>().into())
        })
    }

    /// Freezes or resumes the simulation (menus open/closed).
    pub fn set_paused(&self, paused: bool) {
        let mut st = self.state.borrow_mut();
        st.paused = paused;
        st.input.clear();
    }

    /// Sets the number of laps for the next race (and restarts the current one).
    pub fn set_laps(&self, laps: u32) {
        let mut st = self.state.borrow_mut();
        st.laps = laps.clamp(1, 20);
        st.spawn();
    }

    /// Race HUD state: `{countdown, lap, laps, time, lapTime, best, wrongWay,
    /// finished}` (times in seconds, `best`/`finished` null when unset), or
    /// `null` when no race is running.
    pub fn race_state(&self) -> JsValue {
        let st = self.state.borrow();
        let (Some(race), Some(player)) = (&st.race, st.player()) else { return JsValue::NULL };
        let o = Object::new();
        let set = |k: &str, v: JsValue| {
            let _ = Reflect::set(&o, &k.into(), &v);
        };
        let num = |v: f32| JsValue::from_f64(v as f64);
        set("countdown", num(race.countdown.max(0.0)));
        set("laps", JsValue::from(race.laps));
        set("time", num(race.time));
        set("finished", player.finished.map_or(JsValue::NULL, num));
        set("racers", JsValue::from(st.racers.len() as u32));
        let order = st.standings();
        set("position", JsValue::from(order.iter().position(|&i| i == 0).unwrap_or(0) as u32 + 1));
        let results: Array = order
            .iter()
            .map(|&i| {
                let r = &st.racers[i];
                let e = Object::new();
                let _ = Reflect::set(&e, &"name".into(), &r.model.name.as_str().into());
                let _ = Reflect::set(&e, &"player".into(), &JsValue::from_bool(i == 0));
                let _ = Reflect::set(&e, &"time".into(), &r.finished.map_or(JsValue::NULL, num));
                JsValue::from(e)
            })
            .collect();
        set("standings", results.into());
        if let Some(weapons) = &st.weapons {
            let a = &weapons.armory[0];
            let shown = a.roulette_display().map(|w| (w, true)).or(a.weapon.map(|w| (w, false)));
            set("weapon", shown.map_or(JsValue::NULL, |(w, _)| w.name().into()));
            set("rolling", JsValue::from_bool(shown.is_some_and(|(_, r)| r)));
            set("uses", JsValue::from(a.uses));
            set("bomb", weapons.bomb_fuse(0).map_or(JsValue::NULL, num));
            set("zapped", JsValue::from_bool(a.zap > 0.0));
        }
        match &player.tracker {
            Some(t) => {
                set("lap", JsValue::from((t.laps + 1).min(race.laps)));
                set("lapTime", num(t.lap_time));
                set("best", t.best_lap.map_or(JsValue::NULL, num));
                set("wrongWay", JsValue::from_bool(t.wrong_way && player.finished.is_none()));
            }
            None => set("lap", JsValue::NULL), // arena: no laps
        }
        o.into()
    }

    /// Reads a file by in-game path (e.g. `WAVS/HIT1.WAV`) from the disc.
    pub fn read_game_file(&self, path: String) -> Promise {
        let game = self.game.borrow().clone();
        future_to_promise(async move {
            let game = game.ok_or("no disc open")?;
            let data = game.read(&path).await?;
            Ok(Uint8Array::from(data.as_slice()).into())
        })
    }

    /// Per-frame sound state: `{engine, revs, skid, speed, impact}` or `null`.
    pub fn sound_state(&self) -> JsValue {
        let mut st = self.state.borrow_mut();
        let Some(player) = st.racers.first_mut() else { return JsValue::NULL };
        let engine = player.model.engine_sound;
        let sim = &mut player.sim;
        let o = Object::new();
        let _ = Reflect::set(&o, &"engine".into(), &engine.into());
        let _ = Reflect::set(&o, &"throttle".into(), &JsValue::from_f64(sim.throttle as f64));
        for (k, v) in [("revs", sim.revs()), ("skid", sim.skid()), ("speed", sim.vel.length()), ("impact", sim.take_impact())] {
            let _ = Reflect::set(&o, &k.into(), &JsValue::from_f64(v as f64));
        }
        o.into()
    }

    /// `[first, last]` from the level's `REDBOOK` line (music track numbers
    /// as written in the .inf), or `null` when it has none.
    pub fn level_music(&self) -> JsValue {
        let st = self.state.borrow();
        match st.level.as_ref().and_then(|l| l.info.redbook) {
            Some((a, b)) => Array::of2(&JsValue::from(a), &JsValue::from(b)).into(),
            None => JsValue::NULL,
        }
    }

    /// One-line status for the HUD.
    pub fn status(&self) -> String {
        let st = self.state.borrow();
        let mut parts = Vec::new();
        if let Some(l) = &st.level {
            parts.push(format!("{} ({})", l.info.name, l.id));
        }
        if let Some(p) = st.player() {
            parts.push(p.model.name.clone());
            parts.push(format!("{:.0} mph", p.sim.speed_mph()));
        }
        parts.push(match st.camera.mode {
            CameraMode::Chase => "Chase cam".into(),
            CameraMode::Free => "Free cam".into(),
        });
        parts.push(match &st.gamepad {
            Some(id) => format!("Gamepad: {id}"),
            None => "No gamepad (press a button to connect)".into(),
        });
        parts.join("  |  ")
    }

    /// Every racer's speed in mph (player first), for tests and debugging.
    pub fn racer_speeds(&self) -> Vec<f32> {
        self.state.borrow().racers.iter().map(|r| r.sim.speed_mph()).collect()
    }

    /// Car state for tests and debugging: `{x, y, z, speed_mph, upright}` in
    /// game space, or `null` when no car is on track.
    pub fn car_state(&self) -> JsValue {
        let st = self.state.borrow();
        let Some(sim) = st.player().map(|r| &r.sim) else { return JsValue::NULL };
        let o = Object::new();
        for (k, v) in [("x", sim.pos.x), ("y", sim.pos.y), ("z", sim.pos.z), ("speed_mph", sim.speed_mph())] {
            let _ = Reflect::set(&o, &k.into(), &JsValue::from_f64(v as f64));
        }
        let _ = Reflect::set(&o, &"upright".into(), &JsValue::from_bool(sim.is_upright()));
        o.into()
    }
}

fn entries(list: &[Entry]) -> Array {
    list.iter()
        .map(|e| {
            let o = Object::new();
            let _ = Reflect::set(&o, &"id".into(), &e.id.as_str().into());
            let _ = Reflect::set(&o, &"name".into(), &e.name.as_str().into());
            JsValue::from(o)
        })
        .collect()
}
