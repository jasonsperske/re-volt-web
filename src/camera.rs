//! Viewer cameras: a free-fly camera and the chase camera that follows the
//! player. Works in GL space (Y up).

use glam::{Mat4, Quat, Vec3};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CameraMode {
    Chase,
    Free,
}

/// One frame of free-fly input.
#[derive(Clone, Copy, Default)]
pub struct FlyInput {
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub boost: bool,
}

/// Chase camera framings. `Default` is the original framing; the others
/// are new to this version.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ChaseView {
    Default,
    /// Further back and higher, to see more of the track.
    Far,
    /// Just behind the car and close to the ground, looking ahead.
    Low,
}

impl ChaseView {
    pub const ALL: [ChaseView; 3] = [ChaseView::Default, ChaseView::Far, ChaseView::Low];

    pub fn name(self) -> &'static str {
        match self {
            ChaseView::Default => "Chase cam",
            ChaseView::Far => "Far chase cam",
            ChaseView::Low => "Low chase cam",
        }
    }

    /// Distance behind and height above the car's look point, and how far
    /// ahead of the car the camera aims.
    fn boom(self) -> (f32, f32, f32) {
        match self {
            ChaseView::Default => (230.0, 85.0, 0.0),
            ChaseView::Far => (400.0, 150.0, 0.0),
            ChaseView::Low => (175.0, 30.0, 320.0),
        }
    }
}

/// Look point height above the car origin.
const LOOK_UP: f32 = 25.0;
/// Larger moves than this between frames are teleports (restart, reset).
const SNAP_DISTANCE: f32 = 800.0;

pub struct Camera {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub mode: CameraMode,
    /// Framing used in chase mode.
    pub view: ChaseView,
    /// Extra orbit angle around the car set by the look controls in chase mode.
    pub orbit: f32,
    /// Smoothed chase state: look point, heading and camera offset.
    target: Option<Vec3>,
    heading: Vec3,
    offset: Vec3,
}

impl Camera {
    pub fn new(pos: Vec3, yaw: f32, pitch: f32) -> Self {
        Camera { pos, yaw, pitch, mode: CameraMode::Chase, view: ChaseView::Default, orbit: 0.0, target: None, heading: Vec3::NEG_Z, offset: Vec3::ZERO }
    }

    /// Steps through the chase framings, then the free camera, then back.
    pub fn next_mode(&mut self) {
        let i = ChaseView::ALL.iter().position(|v| *v == self.view).unwrap_or(0);
        match self.mode {
            CameraMode::Chase if i + 1 < ChaseView::ALL.len() => self.view = ChaseView::ALL[i + 1],
            CameraMode::Chase => self.mode = CameraMode::Free,
            CameraMode::Free => {
                self.mode = CameraMode::Chase;
                self.view = ChaseView::Default;
            }
        }
    }

    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            CameraMode::Chase => self.view.name(),
            CameraMode::Free => "Free cam",
        }
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), -self.yaw.cos() * self.pitch.cos())
    }

    /// Free-fly movement.
    pub fn fly(&mut self, c: &FlyInput, dt: f32) {
        self.yaw += c.yaw;
        self.pitch = (self.pitch + c.pitch).clamp(-1.5, 1.5);
        let speed = if c.boost { 6000.0 } else { 1500.0 };
        let right = Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin());
        let dir = self.forward() * c.forward + right * c.right + Vec3::Y * c.up;
        self.pos += dir.clamp_length_max(1.0) * speed * dt;
        self.target = None; // re-snap when returning to the chase camera
    }

    /// Follows a car from behind. `car_pos`/`car_forward` are the car's
    /// (interpolated) pose; `look_yaw` swings the camera round the car and
    /// drifts back when released; `limit` shortens the boom when something
    /// is between the car and the camera.
    ///
    /// Smoothing is split so it removes bounce without making the camera
    /// trail at speed: the look point follows the car exactly sideways but
    /// eases vertically (suspension bob, small bumps); the heading eases so
    /// yaw wobbles don't swing the view; and it's the camera's offset from
    /// the look point that eases, not its world position.
    pub fn chase(&mut self, car_pos: Vec3, car_forward: Vec3, look_yaw: f32, dt: f32, limit: impl Fn(Vec3, Vec3) -> Vec3) {
        let raw_target = car_pos + Vec3::Y * LOOK_UP;
        let flat = Vec3::new(car_forward.x, 0.0, car_forward.z).normalize_or(Vec3::NEG_Z);
        let ease = |rate: f32| 1.0 - (-rate * dt).exp();
        self.orbit = (self.orbit + look_yaw) * (1.0 - ease(1.5));

        let snap = self.target.is_none_or(|t| t.distance(raw_target) > SNAP_DISTANCE);
        let target = match self.target {
            Some(t) if !snap => Vec3::new(raw_target.x, t.y + (raw_target.y - t.y) * ease(5.0), raw_target.z),
            _ => raw_target,
        };
        self.target = Some(target);
        self.heading = if snap { flat } else { self.heading.lerp(flat, ease(6.0)).normalize_or(flat) };

        // Switching framings eases over, like any other change of offset.
        let (boom_back, boom_up, ahead) = self.view.boom();
        let back = Quat::from_rotation_y(-self.orbit) * -self.heading;
        let wanted = back * boom_back + Vec3::Y * boom_up;
        self.offset = if snap { wanted } else { self.offset.lerp(wanted, ease(8.0)) };
        self.pos = limit(target, target + self.offset);
        self.look_at(target - back * ahead);
    }

    /// Puts the camera straight behind the car (e.g. on the start line).
    pub fn snap_behind(&mut self, car_pos: Vec3, car_forward: Vec3) {
        self.target = None;
        self.orbit = 0.0;
        self.chase(car_pos, car_forward, 0.0, 0.0, |_, to| to);
    }

    /// Points the camera at `target` from its current position.
    pub fn look_at(&mut self, target: Vec3) {
        let d = (target - self.pos).normalize_or_zero();
        self.pitch = d.y.clamp(-1.0, 1.0).asin();
        self.yaw = d.x.atan2(-d.z);
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_to_rh(self.pos, self.forward(), Vec3::Y)
    }
}
