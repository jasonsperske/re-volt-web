//! Suspension parts, spinner and aerial, placed as REVOLT.EXE does it
//! (docs/suspension.md). Game space: X right, Y down, Z forward. The
//! original's row-vector frames (rows = local axes) become rotations whose
//! columns are those axes, with any stretch kept as a separate local scale.

use glam::{Mat3, Quat, Vec3};

use crate::formats::params::CarParams;

/// Placement data copied from `PARAMETERS.TXT` at car setup (0x412150).
#[derive(Clone, Debug, Default)]
pub struct PartsSetup {
    /// Offset1 per wheel: the hub pivot the links aim at.
    pub hubs: [Vec3; 4],
    /// Spring mount and rest length per wheel.
    pub springs: [(Vec3, f32); 4],
    /// Axle mount and rest length per wheel.
    pub axles: [(Vec3, f32); 4],
    /// Pin length per wheel (the PIN offset is unused).
    pub pins: [f32; 4],
    /// Spinner offset, axis and angular velocity (rad/s).
    pub spinner: (Vec3, Vec3, f32),
    pub aerial: Option<AerialSetup>,
}

#[derive(Clone, Debug)]
pub struct AerialSetup {
    pub offset: Vec3,
    /// Rest direction, car space, as given (not normalised).
    pub direction: Vec3,
    pub length: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl PartsSetup {
    pub fn new(p: &CarParams) -> Self {
        let aerial = &p.aerial;
        PartsSetup {
            hubs: std::array::from_fn(|n| Vec3::from(p.wheels[n].offset1)),
            springs: std::array::from_fn(|n| (Vec3::from(p.springs[n].offset), p.springs[n].length)),
            axles: std::array::from_fn(|n| (Vec3::from(p.axles[n].offset), p.axles[n].length)),
            pins: std::array::from_fn(|n| p.pins[n].length),
            spinner: (Vec3::from(p.spinner.offset), Vec3::from(p.spinner.axis), p.spinner.ang_vel),
            // Needs both models (0x41bf70).
            aerial: (aerial.section_model.is_some() && aerial.top_model.is_some()).then(|| AerialSetup {
                offset: Vec3::from(aerial.offset),
                direction: Vec3::from(aerial.direction),
                length: aerial.length,
                stiffness: aerial.stiffness,
                damping: aerial.damping,
            }),
        }
    }

    /// Hub point in car space: Offset1 moved by the suspension. Offset2 and
    /// steering are not included (0x426920).
    fn hub(&self, n: usize, susp_pos: f32) -> Vec3 {
        self.hubs[n] + Vec3::Y * susp_pos
    }

    /// Spring: from its mount, Y axis towards the hub, stretched to reach it.
    /// Returns (car-space position, car-space rotation, local scale).
    pub fn spring(&self, n: usize, susp_pos: f32) -> (Vec3, Quat, Vec3) {
        let (mount, length) = self.springs[n];
        let hub = self.hub(n, susp_pos);
        (mount, look_y(mount, hub), Vec3::new(1.0, stretch(mount, hub, length), 1.0))
    }

    /// Axle: from its mount, Z axis towards the hub, stretched to reach it.
    pub fn axle(&self, n: usize, susp_pos: f32) -> (Vec3, Quat, Vec3) {
        let (mount, length) = self.axles[n];
        let hub = self.hub(n, susp_pos);
        (mount, look_z(mount, hub), Vec3::new(1.0, 1.0, stretch(mount, hub, length)))
    }

    /// Pin: at the hub, aligned with the spring, at a fixed scale.
    pub fn pin(&self, n: usize, susp_pos: f32) -> (Vec3, Quat, Vec3) {
        let hub = self.hub(n, susp_pos);
        (hub, look_y(self.springs[n].0, hub), Vec3::new(1.0, -self.pins[n], 1.0))
    }

    /// Spinner rotation after spinning for `angle` radians.
    pub fn spinner(&self, angle: f32) -> (Vec3, Quat) {
        let (offset, axis, _) = self.spinner;
        (offset, Quat::from_axis_angle(axis.normalize_or(Vec3::Y), angle))
    }
}

fn stretch(from: Vec3, to: Vec3, length: f32) -> f32 {
    if length.abs() > 1e-5 { from.distance(to) / length } else { 1.0 }
}

/// Frame with Y along `a → b` and X horizontal (0x443950).
fn look_y(a: Vec3, b: Vec3) -> Quat {
    let d = (b - a).normalize_or(Vec3::Y);
    let r0 = Vec3::new(-d.z, 0.0, d.x).normalize_or(Vec3::X);
    let r2 = r0.cross(d);
    Quat::from_mat3(&Mat3::from_cols(r0, d, r2)).normalize()
}

/// Frame with Z along `a → b` and X horizontal (0x443860).
fn look_z(a: Vec3, b: Vec3) -> Quat {
    let d = (b - a).normalize_or(Vec3::Z);
    let r0 = Vec3::new(d.z, 0.0, -d.x).normalize_or(Vec3::X);
    let r1 = d.cross(r0);
    Quat::from_mat3(&Mat3::from_cols(r0, r1, d)).normalize()
}

/// The whip aerial: three particles in world space, the first fixed to the
/// car, the others pulled back towards the rest direction (0x414480).
#[derive(Clone, Debug)]
pub struct Aerial {
    nodes: [Vec3; 3],
    prev: [Vec3; 3],
    vel: [Vec3; 3],
    impulse: [Vec3; 3],
    /// Grows from 0.2 to 1 after the car appears (car+0xe08).
    grow: f32,
    seed: u32,
}

/// Teleports bigger than this re-straighten the aerial (resets, respawns).
const AERIAL_SNAP: f32 = 300.0;

impl Aerial {
    pub fn new(setup: &AerialSetup, pos: Vec3, rot: Quat) -> Self {
        let mut a = Aerial { nodes: [Vec3::ZERO; 3], prev: [Vec3::ZERO; 3], vel: [Vec3::ZERO; 3], impulse: [Vec3::ZERO; 3], grow: 0.2, seed: 0x2545_f491 };
        a.reset(setup, pos, rot);
        a
    }

    fn section(&self, setup: &AerialSetup) -> f32 {
        setup.length * self.grow
    }

    /// Straight along the rest direction (0x414220).
    pub fn reset(&mut self, setup: &AerialSetup, pos: Vec3, rot: Quat) {
        let base = pos + rot * setup.offset;
        let w = rot * setup.direction;
        let len = self.section(setup);
        for k in 0..3 {
            self.nodes[k] = base + w * (k as f32 * len);
            self.vel[k] = Vec3::ZERO;
            self.impulse[k] = Vec3::ZERO;
        }
        self.prev = self.nodes;
    }

    fn random(&mut self) -> f32 {
        // xorshift; the original uses the C library rand().
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 32768) as f32 / 32767.0
    }

    /// One physics step of `dt` seconds with the body's new pose.
    pub fn step(&mut self, setup: &AerialSetup, pos: Vec3, rot: Quat, body_vel: Vec3, dt: f32) {
        self.prev = self.nodes;
        self.grow = (self.grow + dt).min(1.0);
        let len = self.section(setup);
        let base = pos + rot * setup.offset;
        if base.distance(self.nodes[0]) > AERIAL_SNAP {
            self.reset(setup, pos, rot);
            return;
        }
        self.nodes[0] = base;
        let fallback = setup.direction;
        let constrain = |nodes: &mut [Vec3; 3], k: usize| -> Vec3 {
            let v = nodes[k] - nodes[k - 1];
            let l = v.length();
            let (d, l) = if l <= 1e-5 { (fallback, 1.0) } else { (v / l, l) };
            nodes[k] += d * (len - l);
            d
        };
        let mut w = rot * setup.direction;
        for k in 1..3 {
            // Integrate (no gravity, no resistance).
            self.vel[k] += self.impulse[k];
            self.impulse[k] = Vec3::ZERO;
            self.nodes[k] += self.vel[k] * dt;
            let d = constrain(&mut self.nodes, k);
            // Bend back towards the rest direction, and damp sideways motion.
            let c = d.cross(w);
            let mut force = -(len * setup.stiffness * c.length()) * d.cross(c);
            let mut side = self.vel[k] - self.vel[0];
            side -= w * side.dot(w);
            force -= side * setup.damping;
            self.impulse[k] += force * dt;
            w = d;
            constrain(&mut self.nodes, k);
            // (World collision for the nodes is not ported.)
            let d = constrain(&mut self.nodes, k);
            let along = (self.vel[k] - self.vel[k - 1]).dot(d);
            self.vel[k] -= d * along;
        }
        // Occasional shake of the tip at speed.
        if self.random() < dt * 80.0 {
            let x = dt * 10.0 * body_vel.length_squared().min(1e6);
            if x > 100.0 {
                let right = rot * Vec3::X;
                let mut kick = Vec3::ZERO;
                for j in 0..3 {
                    kick[j] = (0.001 - self.random() * 0.002) * x * right[j];
                }
                self.impulse[2] += kick;
            }
        }
    }

    /// The four drawn segments, interpolated `t` (0..1) of the way from the
    /// previous step: (position, rotation, local scale), section models for
    /// the first three and the top model last (0x4285e0). Segments too
    /// short to see are left out.
    pub fn segments(&self, t: f32) -> Vec<(usize, Vec3, Quat, Vec3)> {
        let p: [Vec3; 3] = std::array::from_fn(|k| self.prev[k].lerp(self.nodes[k], t));
        let mut out = Vec::new();
        let mut last = p[0];
        for i in 1..=4 {
            let t = i as f32 * 0.25;
            let point = p[0] * (2.0 * t * t - 3.0 * t + 1.0) + p[1] * (4.0 * (t - t * t)) + p[2] * (2.0 * t * t - t);
            let seg = point - last;
            if seg.abs().max_element() < 0.1 {
                continue;
            }
            let (a, b, c) = (seg.x, seg.y, seg.z);
            let r0 = if a.abs() < b.abs() {
                if c.abs() > b.abs() { Vec3::new(0.0, c, -b) } else { Vec3::new(-b, a, 0.0) }
            } else if c.abs() > b.abs() {
                Vec3::new(c, 0.0, -a)
            } else {
                Vec3::new(b, -a, 0.0)
            }
            .normalize_or(Vec3::X);
            let len = seg.length();
            let r1 = seg / len;
            let r2 = r0.cross(r1).normalize_or(Vec3::Z);
            let rot = Quat::from_mat3(&Mat3::from_cols(r0, r1, r2)).normalize();
            out.push((i, point, rot, Vec3::new(1.0, len, 1.0)));
            last = point;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rc() -> PartsSetup {
        let mut p = PartsSetup::default();
        p.hubs[0] = Vec3::new(-18.0, 7.0, 42.0);
        p.springs[0] = (Vec3::new(-6.0, -10.0, 41.0), 16.0);
        p.axles[0] = (Vec3::new(-7.0, 7.0, 42.0), 12.0);
        p.pins[0] = -1.35;
        p
    }

    #[test]
    fn spring_reaches_the_hub_and_compresses_with_the_suspension() {
        let p = rc();
        for susp in [-8.0, 0.0, 8.0] {
            let (mount, rot, scale) = p.spring(0, susp);
            // The model's local Y axis, stretched, lands on the hub.
            let tip = mount + rot * (Vec3::Y * scale.y * 16.0);
            assert!(tip.distance(p.hub(0, susp)) < 1e-3, "{susp}: {tip}");
        }
        assert!(p.spring(0, -8.0).2.y < p.spring(0, 8.0).2.y);
    }

    #[test]
    fn axle_points_along_z_to_the_hub() {
        let p = rc();
        let (mount, rot, scale) = p.axle(0, 3.0);
        let tip = mount + rot * (Vec3::Z * scale.z * 12.0);
        assert!(tip.distance(p.hub(0, 3.0)) < 1e-3);
    }

    #[test]
    fn pin_sits_at_the_hub_along_the_spring() {
        let p = rc();
        let (pos, rot, scale) = p.pin(0, 0.0);
        assert_eq!(pos, p.hub(0, 0.0));
        assert!((rot * Vec3::Y).dot((pos - p.springs[0].0).normalize()) > 0.999);
        assert!((scale.y - 1.35).abs() < 1e-6);
    }

    #[test]
    fn aerial_stands_up_at_rest_and_leans_back_when_driving() {
        let setup = AerialSetup { offset: Vec3::new(-5.0, -6.0, -30.0), direction: Vec3::new(0.0, -1.0, 0.0), length: 20.0, stiffness: 2000.0, damping: 5.5 };
        let mut a = Aerial::new(&setup, Vec3::ZERO, Quat::IDENTITY);
        for _ in 0..250 {
            a.step(&setup, Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, 0.008);
        }
        // Fully grown, straight up (-Y): the tip is 2 × Length above the base.
        let base = setup.offset;
        assert!((a.nodes[2] - (base + Vec3::new(0.0, -40.0, 0.0))).length() < 0.5, "{:?}", a.nodes);
        let segs = a.segments(1.0);
        assert_eq!(segs.len(), 4);
        assert!(segs.last().unwrap().1.distance(a.nodes[2]) < 1e-3);
        // Driving forward (+Z) at a steady speed, the tip trails behind the base.
        let mut pos = Vec3::ZERO;
        for _ in 0..250 {
            pos.z += 1000.0 * 0.008;
            a.step(&setup, pos, Quat::IDENTITY, Vec3::new(0.0, 0.0, 1000.0), 0.008);
        }
        let lean = a.nodes[2].z - a.nodes[0].z;
        assert!(lean < -1.0, "tip should trail: {lean}");
        assert!((a.nodes[2] - a.nodes[1]).length() - 20.0 < 0.01);
    }
}
