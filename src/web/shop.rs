//! The toy-shop front end scene: level, props and their animation, camera
//! rig (docs/frontend.md; logic in `crate::frontend`).

use std::collections::{BTreeMap, HashMap};

use glam::{Mat3, Mat4, Quat, Vec3};
use wasm_bindgen::JsValue;

use super::gl::{GpuScene, Renderer};
use super::JsSource;
use crate::assets::GameData;
use crate::collision::{floor_poly, level_world_with, CollisionWorld};
use crate::formats::bmp::Image;
use crate::formats::fob::LevelObject;
use crate::formats::inf::LevelInfo;
use crate::formats::prm::{self, Mesh, Poly, Vertex, POLY_DOUBLE_SIDED};
use crate::frontend::{box_art, CameraRig, CarBox, Clock, NameWheel};
use crate::scene::{self, gl_model, Placement, SceneBuilder};

/// .fob object types used by the shop (§3).
const BIG_SCREEN: i32 = 44;
const CLOCK: i32 = 45;
const CAR_BOX: i32 = 46;
const CUP: i32 = 48;
const NAME_STAND: i32 = 39;
const SMALL_SCREEN: i32 = 53;
const CONE: i32 = 36;

/// Model files by name (LEVELS/FRONTEND unless noted).
const MODELS: [&str; 13] = [
    "BIGSCREEN", "CLOCKBODY", "CLOCKDISC", "CLOCKHANDLARGE", "CLOCKHANDSMALL", "CUP01", "CUP02", "CUP03", "CUP04", "NAMESTAND",
    "NAMEWHEEL", "SMALLSCREEN", "CONE",
];

/// Everything read from the disc, before any GPU upload.
pub struct ShopData {
    level: crate::assets::LevelAssets,
    pages: BTreeMap<i16, Image>,
    meshes: HashMap<&'static str, Mesh>,
    carbox: Mesh,
}

impl ShopData {
    pub async fn load(game: &GameData<JsSource>) -> Result<ShopData, JsValue> {
        let level = game.load_level("FRONTEND").await?;
        // Level pages A..G are 0..6; car box art sheets are pages 27..29.
        let mut pages = BTreeMap::new();
        for (page, letter) in ('A'..='G').enumerate() {
            if let Ok(img) = game.read(&format!("LEVELS/FRONTEND/FRONTEND{letter}.BMP")).await.and_then(|d| crate::formats::bmp::decode(&d)) {
                pages.insert(page as i16, img);
            }
        }
        for k in 1..=3 {
            if let Ok(img) = game.read(&format!("LEVELS/FRONTEND/CARBOX{k}.BMP")).await.and_then(|d| crate::formats::bmp::decode(&d)) {
                pages.insert(26 + k as i16, img);
            }
        }
        let mut meshes = HashMap::new();
        for name in MODELS {
            let path = if name == "CONE" { "MODELS/CONE.M".to_string() } else { format!("LEVELS/FRONTEND/{name}.M") };
            if let Ok(mesh) = game.load_mesh(&path).await {
                meshes.insert(name, mesh);
            }
        }
        let carbox = game.load_mesh("LEVELS/FRONTEND/CARBOX.M").await?;
        Ok(ShopData { level, pages, meshes, carbox })
    }
}

/// A placed object frame from its `.fob` record (rows right = up×look, up, look).
fn object_frame(o: &LevelObject) -> (Vec3, Quat) {
    let up = Vec3::from(o.up).normalize_or(Vec3::Y);
    let look = Vec3::from(o.look).normalize_or(Vec3::Z);
    let right = up.cross(look).normalize_or(Vec3::X);
    (Vec3::from(o.pos), Quat::from_mat3(&Mat3::from_cols(right, up, look)).normalize())
}

struct Placed {
    model: &'static str,
    pos: Vec3,
    rot: Quat,
}

pub struct Shop {
    pub info: LevelInfo,
    pub world: CollisionWorld,
    level: GpuScene,
    models: HashMap<&'static str, GpuScene>,
    statics: Vec<Placed>,
    clock_frame: Option<(Vec3, Quat)>,
    stand_frame: Option<(Vec3, Quat)>,
    big_screen: Option<(Vec3, Quat)>,
    small_screens: Vec<(Vec3, Quat)>,
    screen_big: Option<GpuScene>,
    screen_small: Option<GpuScene>,
    boxes: Vec<(CarBox, GpuScene)>,
    pub rig: CameraRig,
    clock: Clock,
    pub wheel: NameWheel,
    /// Highlighted car (game car index), and whether it's been chosen.
    pub selected_car: Option<i32>,
    pub chosen: bool,
    /// Set when a chosen box lands: (car index, where to drop the car).
    pub drop_request: Option<(i32, Vec3, Quat)>,
}

impl Shop {
    pub fn new(data: ShopData, renderer: &Renderer) -> Result<Shop, JsValue> {
        let textures = renderer.upload_textures(&data.pages)?;
        let level = renderer.upload_shared(&scene::level_scene(&data.level), &textures)?;
        let mut models = HashMap::new();
        for (name, mesh) in &data.meshes {
            let mut b = SceneBuilder::default();
            b.add_mesh(mesh, &Placement::at([0.0; 3]), Some);
            models.insert(*name, renderer.upload_shared(&b, &textures)?);
        }

        let mut statics = Vec::new();
        let (mut clock_frame, mut stand_frame, mut big_screen) = (None, None, None);
        let mut small_screens = Vec::new();
        let mut boxes = Vec::new();
        for o in &data.level.objects {
            let (pos, rot) = object_frame(o);
            match o.kind {
                BIG_SCREEN => {
                    statics.push(Placed { model: "BIGSCREEN", pos, rot });
                    big_screen = Some((pos, rot));
                }
                CLOCK => {
                    statics.push(Placed { model: "CLOCKBODY", pos, rot });
                    clock_frame = Some((pos, rot));
                }
                CUP => {
                    // Models 58 + flags[0]: cup04, cup02, cup01, cup03 (§3).
                    let model = ["CUP04", "CUP02", "CUP01", "CUP03"][o.flags[0].clamp(0, 3) as usize];
                    statics.push(Placed { model, pos, rot });
                }
                NAME_STAND => {
                    statics.push(Placed { model: "NAMESTAND", pos, rot });
                    stand_frame = Some((pos, rot));
                }
                SMALL_SCREEN => {
                    statics.push(Placed { model: "SMALLSCREEN", pos, rot });
                    small_screens.push((pos, rot));
                }
                CONE => statics.push(Placed { model: "CONE", pos, rot }),
                CAR_BOX => {
                    let mesh = box_mesh(&data.carbox, o.flags[0]);
                    let mut b = SceneBuilder::default();
                    b.add_mesh(&mesh, &Placement::at([0.0; 3]), Some);
                    boxes.push((CarBox::new(o.flags[0], pos, rot), renderer.upload_shared(&b, &textures)?));
                }
                _ => {}
            }
        }
        Ok(Shop {
            info: data.level.info.clone(),
            world: level_world_with(&data.level, &counter_top(&data.level)),
            level,
            models,
            statics,
            clock_frame,
            stand_frame,
            big_screen,
            small_screens,
            screen_big: None,
            screen_small: None,
            boxes,
            rig: CameraRig::new(),
            clock: Clock::default(),
            wheel: NameWheel::default(),
            selected_car: None,
            chosen: false,
            drop_request: None,
        })
    }

    /// Shows a track's picture (GFX/<LEVEL>.BMP) on the big and small screens.
    pub fn set_screen_image(&mut self, renderer: &Renderer, img: &Image) -> Result<(), JsValue> {
        for old in [self.screen_big.take(), self.screen_small.take()].into_iter().flatten() {
            renderer.destroy(old);
        }
        let mut tex = BTreeMap::new();
        tex.insert(0i16, img.clone());
        // Quad corners from 0x4be0d8 (big) and 0x4be108 (small), object space.
        let big = [[-299.0, -726.4, -75.2], [299.0, -726.4, -75.2], [299.0, -291.6, 5.0], [-299.0, -291.6, 5.0]];
        let small = [[-129.0, -326.6, -23.1], [132.0, -326.6, -23.1], [132.0, -133.5, -23.1], [-129.0, -133.5, -23.1]];
        let build = |corners: [[f32; 3]; 4], color: u32| {
            let mut b = SceneBuilder::default();
            b.add_mesh(&quad(corners, color), &Placement::at([0.0; 3]), Some);
            b
        };
        self.screen_big = Some(renderer.upload(&build(big, 0xffffffff), &tex)?);
        self.screen_small = Some(renderer.upload(&build(small, 0xffa0a0a0), &tex)?);
        Ok(())
    }

    pub fn update(&mut self, dt: f32) {
        self.rig.update(dt);
        self.clock.update(dt);
        self.wheel.update(dt);
        let cam = self.rig.rot;
        let any_selected = self.selected_car;
        for (b, _) in &mut self.boxes {
            // Cars past 27 use the spare box (−1).
            let selected = match any_selected {
                Some(c) if c > 27 => b.car == -1,
                Some(c) => b.car == c,
                None => false,
            };
            if b.update(selected, selected && self.chosen, cam, dt) {
                let car = any_selected.unwrap_or(0);
                self.drop_request = Some((car, b.pos + Vec3::new(20.0, -10.0, 0.0), b.rot));
            }
        }
    }

    /// Camera view in GL space.
    pub fn view(&self) -> Mat4 {
        let to_gl = scene::to_gl;
        let eye = to_gl(self.rig.pos);
        let fwd = to_gl(self.rig.rot * Vec3::Z);
        let up = to_gl(self.rig.rot * -Vec3::Y);
        Mat4::look_to_rh(eye, fwd, up)
    }

    pub fn eye(&self) -> Vec3 {
        scene::to_gl(self.rig.pos)
    }

    /// Everything to draw this frame.
    pub fn scenes(&self) -> Vec<(&GpuScene, Mat4)> {
        let tau = std::f32::consts::TAU;
        let mut out = vec![(&self.level, Mat4::IDENTITY)];
        let place = |pos: Vec3, rot: Quat, offset: Vec3, spin: f32| {
            // The game's RotZ (matrix_rot_z_revs 0x443390) is built for row
            // vectors, so it turns the opposite way to glam's column-vector
            // rotation; negate to match (name wheel letter, clock direction).
            gl_model(pos + rot * offset, rot * Quat::from_rotation_z(-spin * tau))
        };
        for p in &self.statics {
            if let Some(g) = self.models.get(p.model) {
                out.push((g, gl_model(p.pos, p.rot)));
            }
        }
        if let Some((pos, rot)) = self.clock_frame {
            let hands = Vec3::new(-1.7, -500.0, 37.0);
            for (model, offset, spin) in [
                ("CLOCKHANDSMALL", hands, self.clock.small),
                ("CLOCKHANDLARGE", hands, self.clock.large),
                ("CLOCKDISC", Vec3::new(-1.7, -500.0, 30.0), self.clock.disc),
            ] {
                if let Some(g) = self.models.get(model) {
                    out.push((g, place(pos, rot, offset, spin)));
                }
            }
        }
        if let (Some((pos, rot)), Some(g)) = (self.stand_frame, self.models.get("NAMEWHEEL")) {
            out.push((g, place(pos, rot, Vec3::new(0.0, -490.0, -80.0), self.wheel.angle)));
        }
        if let (Some((pos, rot)), Some(g)) = (self.big_screen, &self.screen_big) {
            out.push((g, gl_model(pos, rot)));
        }
        if let Some(g) = &self.screen_small {
            for &(pos, rot) in &self.small_screens {
                out.push((g, gl_model(pos, rot)));
            }
        }
        for (b, g) in &self.boxes {
            out.push((g, gl_model(b.pos, b.rot)));
        }
        out
    }
}

/// CARBOX.M with its artwork polygon (flag 0x800) pointed at the car's cell.
/// The service counter's top, which the shop's collision leaves out, so a
/// car can stand on it as the workbench.
fn counter_top(level: &crate::assets::LevelAssets) -> Vec<crate::formats::ncp::CollPoly> {
    let [cx, cz] = crate::frontend::COUNTER_CENTER;
    let mut out = Vec::new();
    for m in &level.world.meshes {
        for p in &m.mesh.polys {
            let n = if p.flags & 1 != 0 { 4 } else { 3 };
            let v: Vec<Vec3> = (0..n).map(|i| Vec3::from(m.mesh.verts[p.indices[i] as usize].pos)).collect();
            let c = v.iter().copied().sum::<Vec3>() / n as f32;
            let flat = v.iter().all(|q| (q.y - crate::frontend::COUNTER_TOP).abs() < 1.0);
            if flat && (c.x - cx).hypot(c.z - cz) < crate::frontend::COUNTER_RADIUS {
                out.push(floor_poly(&v));
            }
        }
    }
    out
}

fn box_mesh(carbox: &Mesh, car: i32) -> Mesh {
    let mut mesh = carbox.clone();
    let (page, [u0, v0], size) = box_art(car);
    for p in &mut mesh.polys {
        if p.flags & prm::POLY_WORLD_ENV != 0 {
            p.texture = page;
            // Window corners: (+x, top), (+x, bottom), (−x, bottom), (−x, top)
            // as seen from the front; the art reads left to right from −x.
            let (u1, v1) = (u0 + size, v0 + size);
            p.uvs = [[u1, v0], [u1, v1], [u0, v1], [u0, v0]];
            p.colors = [0xffffffff; 4];
        }
    }
    mesh
}

fn quad(corners: [[f32; 3]; 4], color: u32) -> Mesh {
    let verts = corners.iter().map(|&pos| Vertex { pos, normal: [0.0, 0.0, -1.0] }).collect();
    let poly = Poly {
        flags: prm::POLY_QUAD | POLY_DOUBLE_SIDED,
        texture: 0,
        indices: [0, 1, 2, 3],
        colors: [color; 4],
        // Corners run top-left, top-right, bottom-right, bottom-left on screen.
        uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    };
    Mesh { polys: vec![poly], verts }
}
