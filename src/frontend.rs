//! The toy-shop front end, ported from REVOLT.EXE as documented in
//! docs/frontend.md (section numbers below refer to it). Game space: Y down.
//!
//! Rendering lives in the web layer; this module owns the camera rig and
//! the animation state of the shop props.

use glam::{Mat3, Quat, Vec3};

/// Camera nodes `{position, look target}` (§1.1, table at 0x4c4b78).
pub const NODES: [([f32; 3], [f32; 3]); 24] = [
    ([76.5, -79.3, -4462.6], [109.5, -155.3, -3466.1]),
    ([93.5, -102.9, -1926.7], [110.9, -296.6, -945.7]),
    ([863.5, -1092.5, 512.5], [1491.2, -561.9, 1082.2]),
    ([1355.5, -166.2, 864.4], [2204.6, 360.0, 911.5]),
    ([1589.6, -81.8, -1743.8], [2445.7, -365.5, -1311.8]),
    ([-212.5, -664.6, -706.8], [-212.5, -901.3, 134.4]),
    ([-1773.9, -503.9, -477.2], [-2657.6, -41.1, -546.7]),
    ([-1637.2, -764.6, 206.4], [-2420.9, -224.8, -101.0]),
    ([-1855.2, -733.4, 669.9], [-2503.0, -83.4, 271.3]),
    ([-2007.7, -713.5, 1026.9], [-2575.5, -29.3, 569.3]),
    ([-1171.6, -137.8, 834.4], [-2080.1, -287.1, 444.1]),
    ([-816.2, -534.5, -500.0], [-1591.0, -392.4, -1115.9]),
    ([527.1, -930.5, 1322.3], [1105.5, -375.5, 1922.1]),
    ([-521.2, -2870.0, -325.4], [-306.2, -1894.6, -276.3]),
    ([146.4, -919.2, -206.2], [35.6, 33.0, -78.3]),
    ([-696.8, -493.2, -1728.4], [-1611.2, -510.2, -2133.0]),
    ([2233.9, -507.6, -1393.0], [3125.8, -664.1, -968.6]),
    ([2026.2, -407.2, -1501.5], [2914.3, -579.5, -1075.5]),
    ([-1047.1, -746.2, 1165.8], [-1692.1, -377.2, 1835.1]),
    ([-1047.1, -746.2, 1165.8], [-1692.1, -377.2, 1835.1]),
    ([-1436.8, -612.4, 1353.8], [-1906.3, -354.4, 2198.2]),
    ([-1676.0, -534.9, 1268.1], [-1950.7, -21.6, 2081.1]),
    ([-1052.3, -437.1, 1770.1], [-1926.2, -44.5, 2056.7]),
    ([-1047.1, -746.2, 1165.8], [-1692.1, 0.0, 1835.1]),
];

/// Named nodes for the menus this version has (§2.2).
pub mod node {
    pub const START: usize = 0;
    pub const MAIN: usize = 1;
    pub const SELECT_CAR: usize = 2;
    pub const CAR_CHOSEN: usize = 3;
    pub const SELECT_TRACK: usize = 4;
    pub const OPTIONS: usize = 11;
    pub const ENTER_NAME: usize = 12;
    pub const STUNT_ARENA: usize = 13;
    pub const SELECT_RACE: usize = 14;
    pub const GO: usize = 16;
    pub const SUMMARY: usize = 17;
}

/// Default transition time; the race-start move is 0.25 s (§1.3).
pub const MOVE_TIME: f32 = 0.85;
pub const GO_TIME: f32 = 0.25;

/// Orientation looking from `pos` at `target`: local X right, Y down (the
/// game's "up" row points at the floor), Z forward.
pub fn look_rotation(pos: Vec3, target: Vec3) -> Quat {
    let look = (target - pos).normalize_or(Vec3::Z);
    let right = Vec3::Y.cross(look).normalize_or(Vec3::X);
    let down = look.cross(right);
    Quat::from_mat3(&Mat3::from_cols(right, down, look)).normalize()
}

pub fn node_pose(i: usize) -> (Vec3, Quat) {
    let (p, t) = NODES[i.min(NODES.len() - 1)];
    let (p, t) = (Vec3::from(p), Vec3::from(t));
    (p, look_rotation(p, t))
}

/// Straight-line move plus rotation slerp, eased out with
/// `sin(t·π/2 / duration)` (fe_camera_update 0x48fa30).
pub struct CameraRig {
    pub pos: Vec3,
    pub rot: Quat,
    from: (Vec3, Quat),
    to: (Vec3, Quat),
    pub node: usize,
    t: f32,
    duration: f32,
}

impl CameraRig {
    /// Starts at node 0, holds 0.5 s, then flies to the main menu.
    pub fn new() -> Self {
        let start = node_pose(node::START);
        let mut rig = CameraRig { pos: start.0, rot: start.1, from: start, to: start, node: node::START, t: 0.0, duration: MOVE_TIME };
        rig.go(node::MAIN, MOVE_TIME);
        rig.t = -0.5;
        rig
    }

    pub fn go(&mut self, node: usize, duration: f32) {
        if node == self.node {
            return;
        }
        self.node = node;
        self.from = (self.pos, self.rot);
        let mut to = node_pose(node);
        if self.rot.dot(to.1) < 0.0 {
            to.1 = -to.1; // take the short way round
        }
        self.to = to;
        self.t = 0.0;
        self.duration = duration.max(1e-3);
    }

    pub fn cut(&mut self, node: usize) {
        let pose = node_pose(node);
        (self.pos, self.rot) = pose;
        (self.from, self.to) = (pose, pose);
        self.node = node;
        self.t = self.duration;
    }

    pub fn update(&mut self, dt: f32) {
        self.t = (self.t + dt).min(self.duration);
        let s = (self.t * std::f32::consts::FRAC_PI_2 / self.duration).sin().clamp(0.0, 1.0);
        self.pos = self.from.0.lerp(self.to.0, s);
        self.rot = self.from.1.slerp(self.to.1, s).normalize();
    }

    pub fn arrived(&self) -> bool {
        self.t >= self.duration
    }
}

impl Default for CameraRig {
    fn default() -> Self {
        Self::new()
    }
}

/// Clock hands and disc, in revolutions (§3.3). It doesn't show real time.
#[derive(Default)]
pub struct Clock {
    pub small: f32,
    pub large: f32,
    pub disc: f32,
}

impl Clock {
    pub fn update(&mut self, dt: f32) {
        self.small = (self.small + dt / 60.0).fract();
        self.large = (self.large + dt).fract();
        self.disc = (self.disc - dt / 6.0).rem_euclid(1.0);
    }
}

/// Name wheel (§3.5): 30 positions — A–Z, space, !, delete, done.
pub const WHEEL_SIZE: usize = 30;
pub const WHEEL_DELETE: usize = 28;
pub const WHEEL_DONE: usize = 29;
pub const NAME_MAX: usize = 14;

pub fn wheel_char(index: usize) -> Option<char> {
    match index {
        0..=25 => Some((b'A' + index as u8) as char),
        26 => Some(' '),
        27 => Some('!'),
        _ => None,
    }
}

#[derive(Default)]
pub struct NameWheel {
    pub index: usize,
    /// Rotation in revolutions, eased toward `index / 30`.
    pub angle: f32,
}

impl NameWheel {
    /// The original eases by 1/6 of the remaining turn per AI call with no
    /// dt; this applies the same per 60 Hz frame.
    pub fn update(&mut self, dt: f32) {
        let target = self.index as f32 / WHEEL_SIZE as f32;
        let mut diff = target - self.angle;
        diff -= diff.round(); // wrap into [-0.5, 0.5]
        let k = 1.0 - (1.0f32 - 1.0 / 6.0).powf(dt * 60.0);
        self.angle = (self.angle + diff * k).rem_euclid(1.0);
    }
}

/// Car box animation states (§3.4).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BoxState {
    Home,
    PulledOut,
    ToFloor,
    Dropped,
}

pub struct CarBox {
    /// Car index shown on the box (-1: the unboxed spare).
    pub car: i32,
    pub home_pos: Vec3,
    pub home_rot: Quat,
    pub pos: Vec3,
    pub rot: Quat,
    pub state: BoxState,
    pub timer: f32,
}

/// Where a pulled-out box hovers, and where a chosen box lands (§3.4).
pub fn box_targets() -> (Vec3, Vec3) {
    let (p2, r2) = node_pose(node::SELECT_CAR);
    let (look, right, down) = (r2 * Vec3::Z, r2 * Vec3::X, r2 * Vec3::Y);
    let show = p2 + look * 600.0 + right * 150.0 - down * 50.0;
    let (p3, r3) = node_pose(node::CAR_CHOSEN);
    let look3 = r3 * Vec3::Z;
    let floor = Vec3::new(p3.x + 230.0 * look3.x, 0.0, p3.z + 230.0 * look3.z);
    (show, floor)
}

impl CarBox {
    pub fn new(car: i32, pos: Vec3, rot: Quat) -> Self {
        CarBox { car, home_pos: pos, home_rot: rot, pos, rot, state: BoxState::Home, timer: 0.0 }
    }

    /// `ease_vec3_per_axis`: p += (t - p)·dt·50·k per axis, and
    /// `ease_quat`: q = lerp(q, t, min(1, (1 - s)·dt·30)).
    fn ease(&mut self, to: Vec3, k: Vec3, rot: Quat, s: f32, dt: f32) {
        let step = (k * dt * 50.0).min(Vec3::ONE);
        self.pos += (to - self.pos) * step;
        let mut rot = rot;
        if self.rot.dot(rot) < 0.0 {
            rot = -rot;
        }
        self.rot = self.rot.lerp(rot, ((1.0 - s) * dt * 30.0).min(1.0)).normalize();
    }

    /// Advances the box. `selected` is whether it holds the highlighted car;
    /// `chosen` whether the player confirmed it. Returns true at the moment
    /// the real car should be dropped in.
    pub fn update(&mut self, selected: bool, chosen: bool, camera_rot: Quat, dt: f32) -> bool {
        let (show, floor) = box_targets();
        self.timer += dt;
        match self.state {
            BoxState::Home => {
                self.ease(self.home_pos, Vec3::new(0.2, 0.3, 0.2), self.home_rot, 0.85, dt);
                if selected {
                    self.state = BoxState::PulledOut;
                    self.timer = 0.0;
                }
            }
            BoxState::PulledOut => {
                if !selected {
                    self.state = BoxState::Home;
                    self.timer = 0.0;
                } else if chosen {
                    self.state = BoxState::ToFloor;
                    self.timer = 0.0;
                } else {
                    self.ease(show, Vec3::new(0.2, 0.1, 0.2), camera_rot, 0.9, dt);
                }
            }
            BoxState::ToFloor => {
                self.ease(floor, Vec3::new(0.2, 0.1, 0.2), self.rot, 0.9, dt);
                if self.timer > 1.0 {
                    self.state = BoxState::Dropped;
                    self.timer = 0.0;
                    return true;
                }
            }
            BoxState::Dropped => {
                if self.timer > 0.3 {
                    self.ease(self.home_pos, Vec3::new(0.1, 0.2, 0.1), self.home_rot, 0.9, dt);
                }
                if !chosen {
                    self.state = BoxState::Home;
                }
            }
        }
        false
    }
}

/// The game's car index → car directory, as used by the front-end car boxes
/// (box `.fob` flags[0]). Verified against the box art on CARBOX1–3.BMP.
pub const CAR_ORDER: [&str; 28] = [
    "RC", "MITE", "PHAT", "MOSS", "MUD", "BEATALL", "VOLKEN", "TC6", "DINO", "CANDY", "GENCAR", "TC4", "MOUSE", "FLAG",
    "TC2", "R5", "TC5", "SGT", "TC3", "ADEON", "FONE", "TC1", "ROTOR", "COUGAR", "SUGO", "TOYECA", "AMW", "PANGA",
];

/// Texture page and UV cell of a car's box art (fe_carbox_set_uv 0x42c000).
pub fn box_art(car: i32) -> (i16, [f32; 2], f32) {
    match car {
        0..=26 => {
            let (col, row) = ((car % 3) as f32, ((car % 9) / 3) as f32);
            (27 + (car / 9) as i16, [col * 0.33203125 + 1.0 / 256.0, row * 0.33203125 + 1.0 / 256.0], 0.328125)
        }
        27 => (4, [0.28515625, 0.66796875], 0.328125),
        _ => (6, [0.66796875, 0.0], 0.328125),
    }
}

/// Parses a `Strings\<language>.txt` file: one string per line, keeping only
/// the PC (`\VP`) variant of platform-specific lines and decoding `\nnn`
/// character codes (load_language_strings 0x45fd00).
pub fn parse_strings(data: &[u8]) -> Vec<String> {
    let text: String = data.iter().map(|&b| b as char).collect();
    let mut out = Vec::new();
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        let line = if let Some(rest) = line.strip_prefix("\\VP") {
            rest
        } else if line.starts_with("\\VN") || line.starts_with("\\VS") {
            continue;
        } else {
            line
        };
        let mut s = String::new();
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' && chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                let mut n = 0u32;
                for _ in 0..3 {
                    match chars.peek().and_then(|d| d.to_digit(10)) {
                        Some(d) => {
                            n = n * 10 + d;
                            chars.next();
                        }
                        None => break,
                    }
                }
                s.push(char::from_u32(n).unwrap_or('?'));
            } else {
                s.push(c);
            }
        }
        out.push(s);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_eases_out_and_arrives() {
        let mut rig = CameraRig::new();
        rig.update(0.4); // still holding at node 0
        assert!(rig.pos.distance(Vec3::from(NODES[0].0)) < 1e-3);
        rig.update(0.1 + MOVE_TIME * 0.5);
        let halfway = rig.pos;
        rig.update(MOVE_TIME);
        assert!(rig.arrived());
        assert!(rig.pos.distance(Vec3::from(NODES[1].0)) < 1e-3);
        // Ease-out: more than half the distance covered at half time.
        let (a, b) = (Vec3::from(NODES[0].0), Vec3::from(NODES[1].0));
        assert!(halfway.distance(a) > 0.6 * b.distance(a));
    }

    #[test]
    fn look_rotation_points_at_the_target() {
        let (p, r) = node_pose(node::SELECT_TRACK);
        let t = Vec3::from(NODES[node::SELECT_TRACK].1);
        assert!((r * Vec3::Z).dot((t - p).normalize()) > 0.999);
        // The local Y axis points at the floor (game Y is down).
        assert!((r * Vec3::Y).y > 0.0);
    }

    #[test]
    fn name_wheel_takes_the_short_way() {
        let mut w = NameWheel { index: 29, angle: 0.0 };
        w.update(1.0 / 60.0);
        assert!(w.angle > 0.9, "should turn backwards past zero, got {}", w.angle);
    }

    #[test]
    fn strings_keep_only_pc_variants() {
        let s = parse_strings(b"Re-Volt\r\n\\VNNintendo\r\n\\VPPC line\r\n\\VSSony\r\nCaf\\233\r\n");
        assert_eq!(s, vec!["Re-Volt", "PC line", "Café", ""]);
    }

    #[test]
    fn box_art_cells() {
        assert_eq!(box_art(0), (27, [1.0 / 256.0, 1.0 / 256.0], 0.328125));
        let (page, uv, _) = box_art(13);
        assert_eq!(page, 28);
        assert!((uv[0] - (0.33203125 + 1.0 / 256.0)).abs() < 1e-6);
        assert!((uv[1] - (0.33203125 + 1.0 / 256.0)).abs() < 1e-6);
    }
}
