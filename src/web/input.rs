//! Keyboard, mouse and Gamepad API input, plus the free and chase cameras.

use std::collections::HashSet;

use wasm_bindgen::JsCast;
use web_sys::Gamepad;

const DEADZONE: f32 = 0.15;

// Standard-mapping gamepad buttons.
const PAD_A: usize = 0;
const PAD_B: usize = 1;
const PAD_X: usize = 2;
const PAD_Y: usize = 3;
const PAD_LB: usize = 4;
const PAD_LT: usize = 6;
const PAD_RT: usize = 7;
const PAD_RB: usize = 5;
const PAD_BACK: usize = 8;
const PAD_LSTICK: usize = 10;
const PAD_DLEFT: usize = 14;
const PAD_DRIGHT: usize = 15;

/// Remappable actions, in Controller Settings order.
pub const ACTIONS: [&str; 8] = ["accelerate", "brake", "left", "right", "fire", "flip", "restart", "camera"];
const ACCELERATE: usize = 0;
const BRAKE: usize = 1;
const LEFT: usize = 2;
const RIGHT: usize = 3;
const FIRE: usize = 4;
const FLIP: usize = 5;
const RESTART: usize = 6;
const CAMERA: usize = 7;

/// Keys (`KeyboardEvent.code`) and standard-mapping gamepad buttons for
/// each of [`ACTIONS`]. The left stick always steers as well.
#[derive(Clone, Debug, PartialEq)]
pub struct Bindings {
    pub keys: [Vec<String>; ACTIONS.len()],
    pub pad: [Vec<usize>; ACTIONS.len()],
}

impl Default for Bindings {
    fn default() -> Self {
        let keys = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        Bindings {
            keys: [
                keys(&["KeyW", "ArrowUp"]),
                keys(&["KeyS", "ArrowDown"]),
                keys(&["KeyA", "ArrowLeft"]),
                keys(&["KeyD", "ArrowRight"]),
                keys(&["Space", "Enter"]),
                keys(&["KeyF"]),
                keys(&["KeyR"]),
                keys(&["KeyC"]),
            ],
            pad: [vec![PAD_RT, PAD_A], vec![PAD_LT, PAD_X], vec![PAD_DLEFT], vec![PAD_DRIGHT], vec![PAD_B, PAD_RB], vec![PAD_LB], vec![PAD_Y], vec![PAD_BACK]],
        }
    }
}

#[derive(Default)]
pub struct Input {
    /// Current key and button mapping (Options → Controller Settings).
    pub bindings: Bindings,
    keys: HashSet<String>,
    /// Keys pressed since the last frame (for one-shot actions).
    pressed: HashSet<String>,
    mouse: (f32, f32),
    pad_prev: Vec<bool>,
    /// VR controllers, merged into the next `take`.
    pub xr: XrControls,
}

/// One frame of player intent.
#[derive(Default)]
pub struct Controls {
    // Driving
    pub throttle: f32,
    pub steer: f32,
    // Free camera
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub boost: bool,
    // Both cameras
    pub yaw: f32,
    pub pitch: f32,
    // One-shot actions
    pub reset_car: bool,
    /// Turn the car back onto its wheels (newly pressed).
    pub flip_car: bool,
    pub toggle_camera: bool,
    /// Fire the held weapon (newly pressed).
    pub fire: bool,
    pub gamepad: Option<String>,
}

/// VR controller input for the next frame (see `App::set_xr_input`).
#[derive(Clone, Copy, Default)]
pub struct XrControls {
    pub throttle: f32,
    pub steer: f32,
    pub fire: bool,
    pub flip: bool,
    pub camera: bool,
}

impl Input {
    pub fn key(&mut self, code: &str, down: bool) {
        if down {
            if self.keys.insert(code.to_string()) {
                self.pressed.insert(code.to_string());
            }
        } else {
            self.keys.remove(code);
        }
    }

    pub fn mouse_move(&mut self, dx: f32, dy: f32) {
        self.mouse.0 += dx;
        self.mouse.1 += dy;
    }

    pub fn clear(&mut self) {
        self.keys.clear();
    }

    /// Reads and resets accumulated input. `dt` scales rate-based inputs
    /// (keys, sticks); mouse deltas are already per-frame distances.
    pub fn take(&mut self, dt: f32) -> Controls {
        let k = |c: &str| if self.keys.contains(c) { 1.0f32 } else { 0.0 };
        let b = &self.bindings;
        let held = |a: usize| if b.keys[a].iter().any(|c| self.keys.contains(c)) { 1.0f32 } else { 0.0 };
        let pressed = |a: usize| b.keys[a].iter().any(|c| self.pressed.contains(c));
        // The free camera strafes with the steering keys, but the arrow keys
        // turn it instead.
        let strafe = |a: usize| if b.keys[a].iter().any(|c| !c.starts_with("Arrow") && self.keys.contains(c)) { 1.0f32 } else { 0.0 };
        let mut c = Controls {
            throttle: held(ACCELERATE) - held(BRAKE),
            steer: held(RIGHT) - held(LEFT),
            forward: held(ACCELERATE) - held(BRAKE),
            right: strafe(RIGHT) - strafe(LEFT),
            up: k("KeyE") + k("Space") - k("KeyQ") - k("ControlLeft"),
            boost: self.keys.contains("ShiftLeft") || self.keys.contains("ShiftRight"),
            yaw: (k("ArrowRight") - k("ArrowLeft")) * 2.0 * dt + self.mouse.0 * 0.003,
            pitch: -self.mouse.1 * 0.003,
            reset_car: pressed(RESTART),
            flip_car: pressed(FLIP),
            toggle_camera: pressed(CAMERA),
            fire: pressed(FIRE),
            gamepad: None,
        };
        self.pressed.clear();
        self.mouse = (0.0, 0.0);

        if let Some(pad) = first_gamepad() {
            let axes: Vec<f32> = pad.axes().iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect();
            let axis = |i: usize| {
                let v = axes.get(i).copied().unwrap_or(0.0);
                if v.abs() < DEADZONE { 0.0 } else { (v - DEADZONE * v.signum()) / (1.0 - DEADZONE) }
            };
            let buttons: Vec<f32> = pad
                .buttons()
                .iter()
                .map(|b| b.dyn_into::<web_sys::GamepadButton>().map(|b| b.value() as f32).unwrap_or(0.0))
                .collect();
            let button = |i: usize| buttons.get(i).copied().unwrap_or(0.0);
            let down: Vec<bool> = buttons.iter().map(|&v| v > 0.5).collect();
            let just = |i: usize| down.get(i).copied().unwrap_or(false) && !self.pad_prev.get(i).copied().unwrap_or(false);

            let value = |a: usize| b.pad[a].iter().map(|&i| button(i)).fold(0.0f32, f32::max);
            let tapped = |a: usize| b.pad[a].iter().any(|&i| just(i));
            // Driving: left stick (and the steering buttons) steer.
            c.steer += axis(0) + value(RIGHT) - value(LEFT);
            c.throttle += value(ACCELERATE) - value(BRAKE);
            // Free camera: left stick moves, right stick looks, triggers rise/fall.
            c.forward += -axis(1);
            c.right += axis(0);
            c.up += button(PAD_RT) - button(PAD_LT);
            c.boost |= button(PAD_A) > 0.5 || button(PAD_LSTICK) > 0.5;
            c.yaw += axis(2) * 2.5 * dt;
            c.pitch += -axis(3) * 2.0 * dt;
            c.reset_car |= tapped(RESTART);
            c.flip_car |= tapped(FLIP);
            c.toggle_camera |= tapped(CAMERA);
            c.fire |= tapped(FIRE);
            c.gamepad = Some(pad.id());
            self.pad_prev = down;
        } else {
            self.pad_prev.clear();
        }
        let xr = std::mem::take(&mut self.xr);
        c.throttle += xr.throttle;
        c.steer += xr.steer;
        c.fire |= xr.fire;
        c.flip_car |= xr.flip;
        c.toggle_camera |= xr.camera;
        c.throttle = c.throttle.clamp(-1.0, 1.0);
        c.steer = c.steer.clamp(-1.0, 1.0);
        c
    }
}

fn first_gamepad() -> Option<Gamepad> {
    let pads = web_sys::window()?.navigator().get_gamepads().ok()?;
    pads.iter().find_map(|p| p.dyn_into::<Gamepad>().ok().filter(|g| g.connected()))
}

impl Controls {
    /// The free-camera part of the controls.
    pub fn fly(&self) -> crate::camera::FlyInput {
        crate::camera::FlyInput { forward: self.forward, right: self.right, up: self.up, yaw: self.yaw, pitch: self.pitch, boost: self.boost }
    }
}
