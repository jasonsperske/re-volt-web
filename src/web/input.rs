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

#[derive(Default)]
pub struct Input {
    keys: HashSet<String>,
    /// Keys pressed since the last frame (for one-shot actions).
    pressed: HashSet<String>,
    mouse: (f32, f32),
    pad_prev: Vec<bool>,
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
        let either = |a: &str, b: &str| k(a).max(k(b));
        let mut c = Controls {
            throttle: either("KeyW", "ArrowUp") - either("KeyS", "ArrowDown"),
            steer: either("KeyD", "ArrowRight") - either("KeyA", "ArrowLeft"),
            forward: either("KeyW", "ArrowUp") - either("KeyS", "ArrowDown"),
            right: k("KeyD") - k("KeyA"),
            up: k("KeyE") + k("Space") - k("KeyQ") - k("ControlLeft"),
            boost: self.keys.contains("ShiftLeft") || self.keys.contains("ShiftRight"),
            yaw: (k("ArrowRight") - k("ArrowLeft")) * 2.0 * dt + self.mouse.0 * 0.003,
            pitch: -self.mouse.1 * 0.003,
            reset_car: self.pressed.contains("KeyR"),
            flip_car: self.pressed.contains("KeyF"),
            toggle_camera: self.pressed.contains("KeyC"),
            fire: self.pressed.contains("Space") || self.pressed.contains("Enter"),
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

            // Driving: left stick steers, RT/A accelerate, LT/X brake & reverse.
            c.steer += axis(0);
            c.throttle += button(PAD_RT).max(button(PAD_A)) - button(PAD_LT).max(button(PAD_X));
            // Free camera: left stick moves, right stick looks, triggers rise/fall.
            c.forward += -axis(1);
            c.right += axis(0);
            c.up += button(PAD_RT) - button(PAD_LT);
            c.boost |= button(PAD_A) > 0.5 || button(PAD_LSTICK) > 0.5;
            c.yaw += axis(2) * 2.5 * dt;
            c.pitch += -axis(3) * 2.0 * dt;
            c.reset_car |= just(PAD_Y);
            c.flip_car |= just(PAD_LB);
            c.toggle_camera |= just(PAD_BACK);
            c.fire |= just(PAD_B) || just(PAD_RB);
            c.gamepad = Some(pad.id());
            self.pad_prev = down;
        } else {
            self.pad_prev.clear();
        }
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
