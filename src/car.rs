//! Car physics ported from REVOLT.EXE. See docs/physics.md for the
//! decompiled routines; each section below names the function it follows.
//!
//! Game space: Y down, Z forward, X right. The body is integrated with
//! impulses on a fixed 8 ms step; contact and gravity impulses gathered in
//! one step's collision pass are applied at the start of the next step's
//! move pass, as in the original.
//!
//! Known deviations from the original (see docs/physics.md §8):
//! - Body contacts are solved with projected Gauss-Seidel; the original's
//!   matrix solver (0x4430a0) was not transcribed.
//! - Contacts use a static sphere-vs-polygon test at the new position rather
//!   than the original swept test, and only face contacts (not edges).
//! - Resting/sleep logic, oil zones, car-vs-car and play-mode variants are
//!   omitted.

use glam::{Mat3, Quat, Vec3};

use crate::collision::{CollisionWorld, Contact, NCP_OBJECTS_IGNORE};
use crate::formats::hul::Hull;
use crate::formats::params::CarParams;
use crate::materials;

/// Physics step (physics_fixed_steps_b 0x441580).
pub const DT: f32 = 0.008;
const MAX_STEPS: usize = 10;
/// Global gravity field magnitude along +Y (create_gravity_field 0x44a4b0).
const GRAVITY: f32 = 2200.0;
/// Every damping term in the game scales by `120 * dt`.
const K120: f32 = 120.0 * DT;
/// TopSpeed is stored ×89.4454 at parse time (0x4b0580).
pub const SPEED_SCALE: f32 = 89.4454345703125;

const UP: Vec3 = Vec3::Y; // the car's local "up" axis points at the floor

// Runtime wheel flags (docs/physics.md §2).
const W_PRESENT: u32 = 0x1;
const W_TURNABLE: u32 = 0x2;
const W_POWERED: u32 = 0x4;
const W_SPIN: u32 = 0x10;
const W_SLIDE: u32 = 0x20;
const W_OIL: u32 = 0x80;
const W_FLOOR: u32 = 0x100;
/// Seconds of reduced grip after leaving an oil zone (wheel+0x144).
const OIL_RECOVERY: f32 = 2.5;

/// A slippery circle on the ground (oil slick or water splash), as tested in
/// preprocess_wheel_contacts 0x415bb0: {x, z, radius, height range}.
#[derive(Clone, Copy, Debug)]
pub struct OilZone {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
    /// Game-space y range (y down) the zone applies within.
    pub y_min: f32,
    pub y_max: f32,
}

impl OilZone {
    fn contains(&self, p: Vec3) -> bool {
        p.y >= self.y_min && p.y <= self.y_max && {
            let (dx, dz) = (self.x - p.x, self.z - p.z);
            dx.abs() <= self.radius && dz.abs() <= self.radius && dx * dx + dz * dz <= self.radius * self.radius
        }
    }
}
const W_WALL: u32 = 0x200;
const W_SIDE: u32 = 0x400;
const W_CONTACT: u32 = W_FLOOR | W_WALL | W_SIDE;

#[derive(Clone, Copy, Debug, Default)]
pub struct DriveInput {
    /// -1..1; negative brakes, then reverses once stopped.
    pub throttle: f32,
    /// -1 (left) .. 1 (right).
    pub steer: f32,
}

#[derive(Clone, Debug, Default)]
pub struct WheelSetup {
    pub flags: u32,
    /// Offset1: suspension mount, car space.
    pub axle: Vec3,
    /// Offset1 + Offset2: wheel centre at rest.
    pub center: Vec3,
    pub radius: f32,
    pub inv_mass: f32,
    pub inv_spin_inertia: f32,
    /// Spin torque when airborne or wheelspinning (Mass·15000·Radius).
    pub air_torque: f32,
    pub gravity: f32,
    pub grip: f32,
    pub static_friction: f32,
    pub kinetic_friction: f32,
    pub axle_friction: f32,
    pub steer_ratio: f32,
    pub engine_ratio: f32,
    pub max_pos: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub restitution: f32,
}

#[derive(Clone, Debug)]
pub struct CarSetup {
    pub mass: f32,
    pub inv_mass: f32,
    pub inv_inertia: Mat3,
    pub resistance: f32,
    pub ang_res: f32,
    pub res_mod: f32,
    pub gravity: f32,
    pub hardness: f32,
    pub grip: f32,
    pub static_friction: f32,
    pub kinetic_friction: f32,
    pub steer_rate: f32,
    pub engine_rate: f32,
    /// Units/s.
    pub top_speed: f32,
    pub down_force_mod: f32,
    /// Projectile launch point, car space.
    pub weapon_offset: Vec3,
    pub wheels: [WheelSetup; 4],
    /// Collision spheres (centre in car space, radius).
    pub hull: Vec<(Vec3, f32)>,
}

impl CarSetup {
    /// Mirrors setup_car (0x412150) and setup_wheel_from_info (0x49b320).
    /// Uses the `.hul` spheres when given; otherwise approximates them from
    /// the body mesh bounds.
    pub fn new(p: &CarParams, hull: Option<&Hull>, body_min: Vec3, body_max: Vec3) -> Self {
        let wheels = std::array::from_fn(|n| {
            let (w, s) = (&p.wheels[n], &p.springs[n]);
            let mut flags = 0;
            if w.present {
                flags |= W_PRESENT;
            }
            if w.turnable {
                flags |= W_TURNABLE;
            }
            if w.powered {
                flags |= W_POWERED;
            }
            let mass = w.mass.max(0.01);
            let radius = w.radius.max(1.0);
            let spin_inertia = 0.5 * radius * radius * mass;
            WheelSetup {
                flags,
                axle: Vec3::from(w.offset1),
                center: Vec3::from(w.center()),
                radius,
                inv_mass: 1.0 / mass,
                inv_spin_inertia: 1.0 / spin_inertia,
                air_torque: mass * 15000.0 * radius,
                gravity: w.gravity,
                grip: w.grip,
                static_friction: w.static_friction,
                kinetic_friction: w.kinetic_friction,
                axle_friction: w.axle_friction,
                steer_ratio: w.steer_ratio,
                engine_ratio: w.engine_ratio,
                max_pos: w.max_pos,
                stiffness: s.stiffness,
                damping: s.damping,
                restitution: s.restitution,
            }
        });
        let mass = p.body.mass.max(0.01);
        let inertia = Mat3::from_diagonal(Vec3::from(p.body.inertia).max(Vec3::splat(1.0)));
        let hull = match hull.filter(|h| !h.spheres.is_empty()) {
            Some(h) => h.spheres.iter().map(|s| (Vec3::new(s[0], s[1], s[2]), s[3])).collect(),
            None => {
                let half = (body_max - body_min) * 0.5;
                let mid = (body_max + body_min) * 0.5;
                let r = half.x.min(half.y).max(4.0) * 0.9;
                let mut spheres = Vec::new();
                for sx in [-1.0, 1.0] {
                    for sz in [-1.0, 0.0, 1.0] {
                        spheres.push((mid + Vec3::new(sx * (half.x - r).max(0.0), 0.0, sz * (half.z - r).max(0.0)), r));
                    }
                }
                spheres
            }
        };
        CarSetup {
            mass,
            inv_mass: 1.0 / mass,
            inv_inertia: inertia.inverse(),
            resistance: p.body.resistance,
            ang_res: p.body.ang_res,
            res_mod: p.body.res_mod,
            gravity: p.body.gravity,
            hardness: p.body.hardness,
            grip: p.body.grip,
            static_friction: p.body.static_friction,
            kinetic_friction: p.body.kinetic_friction,
            steer_rate: p.steer_rate,
            engine_rate: p.engine_rate,
            top_speed: p.top_speed * SPEED_SCALE,
            down_force_mod: p.down_force_mod,
            weapon_offset: Vec3::from(p.weapon),
            wheels,
            hull,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WheelState {
    pub flags: u32,
    /// Suspension position along the car's up axis (negative = compressed).
    pub susp_pos: f32,
    pub susp_vel: f32,
    pub susp_impulse: f32,
    /// Spin angle and rate about the axle.
    pub spin: f32,
    pub ang_vel: f32,
    pub ang_impulse: f32,
    pub steer_angle: f32,
    /// World positions of the wheel centre and suspension mount.
    pub center: Vec3,
    pub axle_pos: Vec3,
    pub contact: bool,
    pub material: i32,
    /// Normal impulse from the last contact step (for debugging and HUDs).
    pub load: f32,
    /// Time since the wheel last touched an oil zone (capped at 2.5 s).
    pub oil_timer: f32,
}

/// A wheel–ground contact (record built by wheel_poly_contact 0x419010).
#[derive(Clone, Copy)]
struct WheelContact {
    wheel: usize,
    n: Vec3,
    /// Contact point relative to the body position.
    r: Vec3,
    point: Vec3,
    vel: Vec3,
    depth: f32,
    grip: f32,
    mu_s: f32,
    mu_k: f32,
}

#[derive(Clone, Copy)]
struct BodyContact {
    n: Vec3,
    r: Vec3,
    depth: f32,
    restitution: f32,
    mu_s: f32,
    mu_k: f32,
}

pub struct Car {
    pub setup: CarSetup,
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    pub ang_vel: Vec3,
    /// Current steering (-1..1) and throttle (-1..1) after rate limiting.
    pub steer: f32,
    pub throttle: f32,
    pub wheels: [WheelState; 4],
    impulse: Vec3,
    ang_impulse: Vec3,
    shift: Vec3,
    /// Velocity change during the last integration (B.dVel).
    dvel: Vec3,
    inv_inertia_world: Mat3,
    ang_res_current: f32,
    /// Largest body-contact speed change since `take_impact` (for sounds).
    impact: f32,
    accumulator: f32,
    /// Pose at the start of the latest physics step, for drawing between
    /// steps (`render_pose`).
    prev_pos: Vec3,
    prev_rot: Quat,
    prev_wheel_centers: [Vec3; 4],
    scratch: Vec<Contact>,
    /// Slippery zones currently on the track.
    oil_zones: Vec<OilZone>,
    /// Weapon effects (docs/weapons.md): electro zap cuts the throttle and
    /// slows steering; turbo raises top speed and overrides grip/downforce.
    pub zapped: bool,
    pub top_speed_scale: f32,
    pub friction_override: Option<f32>,
    pub down_force_override: Option<f32>,
}

impl Car {
    pub fn new(setup: CarSetup, pos: Vec3, rot: Quat) -> Self {
        let ang_res = setup.ang_res;
        let mut car = Car {
            setup,
            pos,
            rot,
            vel: Vec3::ZERO,
            ang_vel: Vec3::ZERO,
            steer: 0.0,
            throttle: 0.0,
            wheels: [WheelState::default(); 4],
            impulse: Vec3::ZERO,
            ang_impulse: Vec3::ZERO,
            shift: Vec3::ZERO,
            dvel: Vec3::ZERO,
            inv_inertia_world: Mat3::IDENTITY,
            ang_res_current: ang_res,
            impact: 0.0,
            accumulator: 0.0,
            prev_pos: pos,
            prev_rot: rot,
            prev_wheel_centers: [pos; 4],
            scratch: Vec::new(),
            oil_zones: Vec::new(),
            zapped: false,
            top_speed_scale: 1.0,
            friction_override: None,
            down_force_override: None,
        };
        car.place(pos, rot);
        car
    }

    pub fn place(&mut self, pos: Vec3, rot: Quat) {
        self.pos = pos;
        self.rot = rot;
        self.vel = Vec3::ZERO;
        self.ang_vel = Vec3::ZERO;
        self.impulse = Vec3::ZERO;
        self.ang_impulse = Vec3::ZERO;
        self.shift = Vec3::ZERO;
        self.dvel = Vec3::ZERO;
        self.steer = 0.0;
        self.throttle = 0.0;
        self.update_inertia();
        for n in 0..4 {
            self.wheels[n] = WheelState { flags: self.setup.wheels[n].flags, oil_timer: OIL_RECOVERY, ..Default::default() };
            self.place_wheel(n);
        }
        self.save_prev_pose();
    }

    /// Places the car at rest on the ground below (or slightly above) `pos`.
    /// Level start positions don't account for each car's ride height.
    pub fn place_on_ground(&mut self, world: &CollisionWorld, pos: Vec3, rot: Quat) {
        let down = rot * UP;
        let ground = world.raycast(pos - down * 50.0, down, 1000.0, NCP_OBJECTS_IGNORE).map(|h| h.point);
        let pos = match ground {
            Some(g) => {
                let ride = self
                    .setup
                    .wheels
                    .iter()
                    .filter(|w| w.flags & W_PRESENT != 0)
                    .map(|w| w.center.y + w.radius)
                    .fold(0.0f32, f32::max);
                g - down * (ride + 1.0)
            }
            None => pos,
        };
        self.place(pos, rot);
    }

    pub fn forward(&self) -> Vec3 {
        self.rot * Vec3::Z
    }

    pub fn up(&self) -> Vec3 {
        self.rot * UP
    }

    pub fn forward_speed(&self) -> f32 {
        self.vel.dot(self.forward())
    }

    /// Speed in the units the game's TopSpeed parameter uses.
    pub fn speed_mph(&self) -> f32 {
        self.vel.length() / SPEED_SCALE
    }

    pub fn is_upright(&self) -> bool {
        self.up().dot(Vec3::Y) > 0.3
    }

    /// Engine revs as a fraction of top speed: mean |ω·R| over powered
    /// wheels (the game keeps the same mean at car+0xdfc for engine sound).
    pub fn revs(&self) -> f32 {
        let powered: Vec<usize> = (0..4).filter(|&n| self.setup.wheels[n].flags & (W_PRESENT | W_POWERED) == (W_PRESENT | W_POWERED)).collect();
        if powered.is_empty() || self.setup.top_speed <= 0.0 {
            return 0.0;
        }
        let sum: f32 = powered.iter().map(|&n| (self.wheels[n].ang_vel * self.setup.wheels[n].radius).abs()).sum();
        sum / powered.len() as f32 / self.setup.top_speed
    }

    /// Fraction of grounded wheels that are sliding or wheelspinning.
    pub fn skid(&self) -> f32 {
        let grounded: Vec<&WheelState> = self.wheels.iter().filter(|w| w.flags & W_CONTACT != 0).collect();
        if grounded.is_empty() {
            return 0.0;
        }
        grounded.iter().filter(|w| w.flags & (W_SLIDE | W_SPIN) != 0).count() as f32 / grounded.len() as f32
    }

    /// Hardest body impact (speed change, units/s) since the last call.
    pub fn take_impact(&mut self) -> f32 {
        std::mem::take(&mut self.impact)
    }

    pub fn wheel_present(&self, n: usize) -> bool {
        self.setup.wheels[n].flags & W_PRESENT != 0
    }

    /// Wheel centre and orientation (steer, then spin) in game space.
    pub fn wheel_transform(&self, n: usize) -> (Vec3, Quat) {
        let w = &self.wheels[n];
        (w.center, self.rot * Quat::from_rotation_y(w.steer_angle) * Quat::from_rotation_x(w.spin))
    }

    /// Advances by `dt` seconds in fixed 8 ms steps, at most 10 per call,
    /// carrying the remainder (physics_fixed_steps_b).
    pub fn update(&mut self, world: &CollisionWorld, input: DriveInput, dt: f32) {
        self.accumulator += dt;
        let mut steps = 0;
        while self.accumulator >= DT && steps < MAX_STEPS {
            self.step(world, input);
            self.accumulator -= DT;
            steps += 1;
        }
        if steps == MAX_STEPS {
            self.accumulator = self.accumulator.min(DT);
        }
    }

    /// Queues an impulse on the body for the next physics step.
    pub fn add_impulse(&mut self, j: Vec3) {
        self.impulse += j;
    }

    /// Queues an impulse applied at a world point (adds the matching torque).
    pub fn add_impulse_at(&mut self, point: Vec3, j: Vec3) {
        self.impulse += j;
        self.ang_impulse += (point - self.pos).cross(j);
    }

    pub fn add_angular_impulse(&mut self, a: Vec3) {
        self.ang_impulse += a;
    }

    /// Body roll inertia about its local X axis (the game's Ixx).
    pub fn inertia_xx(&self) -> f32 {
        1.0 / self.setup.inv_inertia.x_axis.x.max(1e-9)
    }

    /// World-space box around the hull spheres.
    pub fn aabb(&self) -> (Vec3, Vec3) {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for &(c, r) in &self.setup.hull {
            let w = self.pos + self.rot * c;
            lo = lo.min(w - r);
            hi = hi.max(w + r);
        }
        if lo.x > hi.x {
            (self.pos - 30.0, self.pos + 30.0)
        } else {
            (lo, hi)
        }
    }

    /// Hull spheres in world space.
    pub fn world_spheres(&self) -> impl Iterator<Item = (Vec3, f32)> + '_ {
        self.setup.hull.iter().map(|&(c, r)| (self.pos + self.rot * c, r))
    }

    /// Sets the oil/water zones the wheels test against.
    pub fn set_oil_zones(&mut self, zones: &[OilZone]) {
        self.oil_zones.clear();
        self.oil_zones.extend_from_slice(zones);
    }

    /// Runs exactly one 8 ms physics step (for tests and tracing).
    pub fn step_once(&mut self, world: &CollisionWorld, input: DriveInput) {
        self.step(world, input);
    }

    fn save_prev_pose(&mut self) {
        self.prev_pos = self.pos;
        self.prev_rot = self.rot;
        for n in 0..4 {
            self.prev_wheel_centers[n] = self.wheels[n].center;
        }
    }

    /// How far the clock is into the next physics step (0..1).
    fn step_fraction(&self) -> f32 {
        (self.accumulator / DT).clamp(0.0, 1.0)
    }

    /// Pose to draw: interpolated between the last two physics steps. Frames
    /// (16.7 ms) don't divide evenly into 8 ms steps, so drawing the raw
    /// pose advances the car 2 steps on most frames and 3 on some, which
    /// shows as judder.
    pub fn render_pose(&self) -> (Vec3, Quat) {
        let t = self.step_fraction();
        (self.prev_pos.lerp(self.pos, t), self.prev_rot.slerp(self.rot, t))
    }

    /// Interpolated wheel transform (see `render_pose`).
    pub fn render_wheel_transform(&self, n: usize) -> (Vec3, Quat) {
        let t = self.step_fraction();
        let (center, _) = self.wheel_transform(n);
        let (_, rot) = self.render_pose();
        let w = &self.wheels[n];
        (self.prev_wheel_centers[n].lerp(center, t), rot * Quat::from_rotation_y(w.steer_angle) * Quat::from_rotation_x(w.spin))
    }

    fn step(&mut self, world: &CollisionWorld, input: DriveInput) {
        self.save_prev_pose();
        self.control(input);
        // car_move_handler 0x469720
        self.update_body();
        for n in 0..4 {
            if self.wheel_present(n) {
                self.update_wheel(n);
            }
        }
        // car_coll_handler 0x46af00
        let contacts = self.detect_wheel_contacts(world);
        let contacts = self.preprocess_wheel_contacts(contacts);
        self.process_wheel_contacts(&contacts);
        self.side_downforce();
        self.impulse += Vec3::Y * (self.setup.mass * GRAVITY * DT);
        self.body_world_collision(world);
        self.set_ang_resistance();
    }

    /// car_control_input 0x41e750: rate-limited steering and throttle.
    fn control(&mut self, input: DriveInput) {
        let s = input.steer.clamp(-1.0, 1.0);
        let sign = |v: f32| if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 };
        let mut rate = self.setup.steer_rate * DT;
        if self.zapped {
            rate /= 3.0; // car_control_input: steering slowed while zapped
        }
        let same = s != 0.0 && sign(s) == sign(self.steer);
        // Steering further is slower; re-centring or reversing is 4× faster.
        rate *= if same { 0.5 } else { 2.0 };
        if !same {
            rate *= 2.0;
        }
        self.steer = approach(self.steer, s, rate);

        // Zapped cars get no power (throttle byte forced to 0).
        let t = if self.zapped { 0.0 } else { input.throttle.clamp(-1.0, 1.0) };
        if (t < 0.0 && self.throttle > 0.0) || (t > 0.0 && self.throttle < 0.0) {
            self.throttle = 0.0;
        }
        self.throttle = approach(self.throttle, t, self.setup.engine_rate * DT);
    }

    fn update_inertia(&mut self) {
        let r = Mat3::from_quat(self.rot);
        self.inv_inertia_world = r * self.setup.inv_inertia * r.transpose();
    }

    /// update_particle 0x47aa50 + update_body 0x40c6a0.
    fn update_body(&mut self) {
        let s = &self.setup;
        self.pos += self.shift;
        self.shift = Vec3::ZERO;
        let old_vel = self.vel;
        self.vel += self.impulse * s.inv_mass;
        self.impulse = Vec3::ZERO;
        self.vel *= 1.0 - K120 * s.resistance;
        self.pos += self.vel * DT;
        self.dvel = self.vel - old_vel;

        self.ang_vel += self.inv_inertia_world * self.ang_impulse;
        self.ang_impulse = Vec3::ZERO;
        let scale = 1.0 - self.ang_res_current * K120;
        self.ang_vel = if scale > 0.0 { self.ang_vel * scale } else { Vec3::ZERO };
        let w = self.ang_vel;
        let spin = Quat::from_xyzw(w.x, w.y, w.z, 0.0) * self.rot;
        let q = self.rot + spin * (0.5 * DT);
        if q.length() > 1e-5 {
            self.rot = q.normalize();
        }
        self.update_inertia();
    }

    /// update_car_wheel 0x415350: suspension and spin as 1-D bodies.
    fn update_wheel(&mut self, n: usize) {
        let ws = &self.setup.wheels[n];
        let up = self.up();
        let throttle = self.throttle;
        let (pos, ang_vel, dvel) = (self.pos, self.ang_vel, self.dvel);
        let w = &mut self.wheels[n];
        let contact = w.flags & W_CONTACT != 0;
        if (ws.flags & W_POWERED != 0 && !contact) || (w.flags & W_SPIN != 0 && contact) {
            let t = throttle * ws.air_torque * DT;
            w.ang_impulse += if ws.engine_ratio < 0.0 { -t } else { t };
        }
        let mut d_susp = ws.inv_mass * w.susp_impulse;
        let d_ang = ws.inv_spin_inertia * w.ang_impulse;
        // The suspension feels the body's rotation and acceleration.
        let r = w.axle_pos - pos;
        let c = r.cross(ang_vel);
        d_susp += DT * ang_vel.cross(c).dot(up);
        d_susp -= dvel.dot(up);
        w.susp_vel += d_susp;
        w.ang_vel += d_ang;
        w.ang_vel *= 1.0 - K120 * ws.axle_friction;
        w.susp_pos = (w.susp_pos + w.susp_vel * DT).clamp(-ws.max_pos, ws.max_pos);
        w.spin = (w.spin + w.ang_vel * DT).rem_euclid(std::f32::consts::TAU);
        let f = spring_force(ws, w.susp_pos, w.susp_vel);
        w.susp_vel += f * DT * ws.inv_mass * if contact { 0.5 } else { 1.0 };
        w.susp_impulse = 0.0;
        w.ang_impulse = 0.0;
        self.place_wheel(n);
    }

    /// Wheel world positions and steering from the body and suspension.
    fn place_wheel(&mut self, n: usize) {
        let ws = &self.setup.wheels[n];
        let susp = UP * self.wheels[n].susp_pos;
        let center = self.pos + self.rot * (ws.center + susp);
        let axle_pos = self.pos + self.rot * (ws.axle + susp);
        // Right wheels copy the left wheel's steering (odd copies n-1).
        let steer_from = if n % 2 == 1 { n - 1 } else { n };
        let sw = &self.setup.wheels[steer_from];
        // The game rotates by Steer·SteerRatio with RotY(a) on row vectors,
        // which is -a about Y for our column-vector quaternions.
        let angle = if sw.flags & W_TURNABLE != 0 { -self.steer * sw.steer_ratio } else { 0.0 };
        let w = &mut self.wheels[n];
        w.center = center;
        w.axle_pos = axle_pos;
        w.steer_angle = angle;
    }

    /// car_world_coll_detect 0x418be0 / wheel_poly_contact 0x419010.
    fn detect_wheel_contacts(&mut self, world: &CollisionWorld) -> Vec<WheelContact> {
        let mut out = Vec::new();
        let up = self.up();
        for n in 0..4 {
            // Oil: the timer restarts while in a zone and recovers otherwise.
            let w = &mut self.wheels[n];
            w.oil_timer = if w.flags & W_OIL != 0 { 0.0 } else { (w.oil_timer + DT).min(OIL_RECOVERY) };
            // Clear wheelspin, sliding, oil and contact bits.
            self.wheels[n].flags &= !(0x8 | W_SPIN | W_SLIDE | W_OIL | W_CONTACT);
            self.wheels[n].contact = false;
            self.wheels[n].load = 0.0;
            if !self.wheel_present(n) {
                continue;
            }
            let (center, radius) = (self.wheels[n].center, self.setup.wheels[n].radius);
            world.sphere_contacts(center, radius, NCP_OBJECTS_IGNORE, &mut self.scratch);
            for c in &self.scratch {
                let r = c.point - self.pos;
                let mat = materials::get(c.material);
                // The stored contact velocity is the body's; the wheel's own
                // suspension speed only decides whether it is approaching.
                let vel = self.vel + self.ang_vel.cross(r);
                if (vel + up * self.wheels[n].susp_vel).dot(c.normal) > 0.0 {
                    continue; // separating
                }
                let vel = vel - mat.surface_velocity();
                let ws = &self.setup.wheels[n];
                let depth = -c.depth + mat.corrugation_depth(c.point + c.normal * 2.0);
                if self.oil_zones.iter().any(|z| z.contains(c.point)) {
                    self.wheels[n].flags |= W_OIL;
                }
                // Reduced grip while recovering from oil.
                let t = self.wheels[n].oil_timer;
                let oil = if t < OIL_RECOVERY { (t + 0.2) * 0.5 / 2.7 } else { 1.0 };
                // Turbo sets both wheel frictions to 2.0 (turbo_move 0x498af0).
                let (fs, fk) = match self.friction_override {
                    Some(f) => (f, f),
                    None => (ws.static_friction, ws.kinetic_friction),
                };
                out.push(WheelContact {
                    wheel: n,
                    n: c.normal,
                    r,
                    point: c.point,
                    vel,
                    depth,
                    grip: ws.grip * mat.grip,
                    mu_s: fs * mat.roughness * oil,
                    mu_k: fk * mat.roughness * oil,
                });
                self.wheels[n].material = c.material;
            }
        }
        out
    }

    /// preprocess_wheel_contacts 0x415bb0: per wheel, drop near-duplicate
    /// contacts (within 3 units in x or z, keeping the deeper) and push the
    /// wheel out of the ground. The suspension absorbs what it can along the
    /// car's up axis; the rest moves the body.
    fn preprocess_wheel_contacts(&mut self, mut contacts: Vec<WheelContact>) -> Vec<WheelContact> {
        let mut keep = vec![true; contacts.len()];
        for i in 0..contacts.len() {
            for j in i + 1..contacts.len() {
                let (a, b) = (&contacts[i], &contacts[j]);
                if !keep[i] || !keep[j] || a.wheel != b.wheel {
                    continue;
                }
                if (a.point.x - b.point.x).abs() < 3.0 || (a.point.z - b.point.z).abs() < 3.0 {
                    if a.depth < b.depth { keep[j] = false } else { keep[i] = false }
                }
            }
        }
        let mut k = keep.iter();
        contacts.retain(|_| *k.next().unwrap());

        let up = self.up();
        let mut push = [Vec3::ZERO; 4];
        for c in contacts.iter().filter(|c| c.depth < 0.0) {
            push[c.wheel] += c.n * -c.depth;
        }
        for n in 0..4 {
            if push[n] == Vec3::ZERO {
                continue;
            }
            let max = self.setup.wheels[n].max_pos;
            let w = &mut self.wheels[n];
            let mut absorbed = push[n].dot(up);
            w.susp_pos += absorbed;
            if w.susp_pos > max {
                absorbed -= w.susp_pos - max;
                w.susp_pos = max;
            } else if w.susp_pos < -max {
                absorbed -= w.susp_pos + max;
                w.susp_pos = -max;
            }
            self.shift += push[n] - up * absorbed;
        }
        contacts
    }

    /// process_wheel_contacts 0x416900.
    fn process_wheel_contacts(&mut self, contacts: &[WheelContact]) {
        let up = self.up();
        let mut j_sum = Vec3::ZERO;
        let mut torque = Vec3::ZERO;
        for c in contacts {
            let vn = c.vel.dot(c.n);
            let n_up = c.n.dot(up);
            if c.depth < 0.0 {
                let ws = &self.setup.wheels[c.wheel];
                let w = &mut self.wheels[c.wheel];
                w.susp_vel = -(n_up * vn);
                w.susp_vel += up.y * ws.gravity * DT;
            }
            let j = self.wheel_contact_impulse(c, vn, n_up);
            j_sum += j;
            torque += c.r.cross(j);
        }
        self.ang_impulse += torque;
        self.impulse += j_sum;
    }

    /// wheel_contact_impulse 0x416b80: the tyre model.
    fn wheel_contact_impulse(&mut self, c: &WheelContact, vn: f32, n_up: f32) -> Vec3 {
        let up = self.up();
        let (n, i) = (c.n, c.wheel);
        let steer_from = if i % 2 == 1 { i - 1 } else { i };
        let right = self.rot * Quat::from_rotation_y(self.wheels[steer_from].steer_angle) * Vec3::X;
        let ws = self.setup.wheels[i].clone();
        let denom = self.setup.inv_mass + n.dot((self.inv_inertia_world * c.r.cross(n)).cross(c.r));
        let top_speed = self.setup.top_speed * self.top_speed_scale;
        let (mass, gravity, throttle) = (self.setup.mass, self.setup.gravity, self.throttle);
        let center = self.wheels[i].center;
        let w = &mut self.wheels[i];

        // Rolling direction on the contact plane.
        let mut fwd = n.cross(right);
        let len = fwd.length();
        let grip_k;
        if len > 0.7 {
            fwd /= len;
            grip_k = 1.0;
            w.flags |= if n.y.abs() > 0.5 { W_FLOOR } else { W_WALL };
        } else if n.y.abs() > 0.5 {
            grip_k = 1.0;
            w.flags |= W_FLOOR | W_SIDE;
        } else {
            grip_k = len / 4.0 + 0.1;
            w.flags |= W_WALL | W_SIDE;
        }
        w.contact = true;

        // Normal impulse: cancel approach speed, softened by the spring's
        // restitution, plus the spring's own push.
        let mut normal = if n_up < 0.9 { -vn / denom } else { 0.0 };
        normal += ws.restitution * n_up * n_up * normal;
        let d = center - self.pos - c.r;
        let mut spring = 0.0;
        if sign(w.susp_pos) == sign(d.dot(up)) {
            spring = spring_force(&ws, w.susp_pos, w.susp_vel) * DT * n_up;
            normal -= spring;
        }
        if normal < 0.0 {
            return Vec3::ZERO;
        }
        w.load = normal;
        let jn = n * normal;

        // Lateral slip friction.
        let vt = c.vel - n * vn;
        let vs = vt - fwd * fwd.dot(vt);
        let mut f = vs * (c.grip * spring.abs() * K120 * -0.35);

        // Drive and braking along the rolling direction.
        let mut drive = 0.0;
        if ws.flags & W_POWERED != 0 {
            let t = throttle * ws.engine_ratio * DT;
            drive = t - t.abs() * (w.ang_vel * ws.radius / top_speed);
            if sign(drive) != sign(t) {
                drive = 0.0;
            }
        }
        if throttle.abs() < 0.01 || sign(throttle) == -sign(w.ang_vel) {
            drive -= ws.axle_friction * w.ang_vel * ws.radius * K120;
        }
        if drive.abs() > (ws.radius * grip_k * c.mu_s * normal).abs() {
            w.flags |= W_SPIN;
        }
        f += fwd * (drive / ws.radius);

        // Friction limits: absolute cap, then static/kinetic.
        let mut mag = f.length();
        let cap = mass * DT * gravity * 4.0;
        if mag > cap {
            f *= cap / mag;
            mag = cap;
            w.flags |= W_SLIDE;
        }
        if mag > c.mu_s * grip_k * normal {
            f *= c.mu_k * grip_k * normal / mag;
            w.flags |= W_SLIDE;
        }

        // Spin the wheel toward its rolling speed unless it's wheelspinning.
        if w.flags & W_SPIN == 0 {
            let target = fwd.dot(c.vel) / ws.radius;
            w.ang_vel += (target - w.ang_vel) * (K120 / 4.0);
        }

        let out = jn + f;
        Vec3::new(snap(out.x), snap(out.y), snap(out.z))
    }

    /// car_side_downforce 0x41cc20: only when exactly the left or right pair
    /// of wheels is touching.
    fn side_downforce(&mut self) {
        let mut mask = 0;
        for n in 0..4 {
            if self.wheel_present(n) && self.wheels[n].flags & W_CONTACT != 0 {
                mask |= 1 << n;
            }
        }
        if mask != 0xF && (mask == 0x5 || mask == 0xA) {
            let up = self.up();
            let k = (1.5 - up.y.abs()).min(1.0);
            let v = DT * k * self.vel.dot(self.forward());
            let modifier = self.down_force_override.unwrap_or(self.setup.down_force_mod);
            self.impulse += up * (modifier * v);
        }
    }

    /// car_body_coll_step 0x46ac60: hull spheres against the world.
    fn body_world_collision(&mut self, world: &CollisionWorld) {
        let mut contacts: Vec<BodyContact> = Vec::new();
        for &(local, radius) in &self.setup.hull {
            let center = self.pos + self.rot * local;
            world.sphere_contacts(center, radius, NCP_OBJECTS_IGNORE, &mut self.scratch);
            for c in &self.scratch {
                let r = c.point - self.pos;
                let mat = materials::get(c.material);
                let vel = self.vel + self.ang_vel.cross(r) - mat.surface_velocity();
                if vel.dot(c.normal) >= 0.0 {
                    continue;
                }
                let mut bc = BodyContact {
                    n: c.normal,
                    r,
                    depth: -c.depth + mat.corrugation_depth(c.point + c.normal * 2.0),
                    restitution: mat.hardness * self.setup.hardness,
                    mu_s: mat.roughness * self.setup.static_friction,
                    mu_k: mat.roughness * self.setup.kinetic_friction,
                };
                if c.normal.y.abs() < 0.15 {
                    // Walls are slippery and a little bouncier.
                    bc.mu_s *= 0.1;
                    bc.mu_k *= 0.1;
                    bc.restitution += 0.1;
                }
                contacts.push(bc);
            }
        }
        if contacts.is_empty() {
            return;
        }

        // body_contact_pushout 0x40cd90 (merging overlapping contacts by
        // pushing only as far as each normal still needs).
        let mut shift = Vec3::ZERO;
        for c in contacts.iter().filter(|c| c.depth < 0.0) {
            let needed = -c.depth - shift.dot(c.n);
            if needed > 0.0 {
                shift += c.n * needed;
            }
        }
        self.shift += shift;

        // body_resolve_contacts 0x40d2a0: non-negative normal impulses that
        // stop approach at every contact, accounting for impulses already
        // queued this step (gravity, wheels).
        let m = self.setup.inv_mass;
        let ii = self.inv_inertia_world;
        let queued_v = self.vel + self.impulse * m;
        let queued_w = self.ang_vel + ii * self.ang_impulse;
        let k = contacts.len();
        let a = |i: usize, j: usize| {
            let (ci, cj) = (&contacts[i], &contacts[j]);
            ci.n.dot(cj.n * m + (ii * cj.r.cross(cj.n)).cross(ci.r))
        };
        let b: Vec<f32> = contacts
            .iter()
            .map(|c| {
                let v = queued_v + queued_w.cross(c.r);
                (-(1.0 + c.restitution) * v.dot(c.n)).max(0.0)
            })
            .collect();
        let mut lambda = vec![0.0f32; k];
        for _ in 0..(2 * k).max(8) {
            for i in 0..k {
                let mut rest = b[i];
                for j in 0..k {
                    if j != i {
                        rest -= a(i, j) * lambda[j];
                    }
                }
                lambda[i] = (rest / a(i, i).max(1e-6)).max(0.0);
            }
        }

        // Track the hardest hit, as the game does for impact sounds (B+0x208).
        let hardest = lambda.iter().fold(0.0f32, |a, &l| a.max(l * m));
        self.impact = self.impact.max(hardest);
        let mut j_sum = Vec3::ZERO;
        let mut torque = Vec3::ZERO;
        for (c, &l) in contacts.iter().zip(&lambda) {
            let mut j = c.n * l;
            let vel = self.vel + self.ang_vel.cross(c.r);
            self.contact_friction(&mut j, c, vel);
            j_sum += j;
            torque += c.r.cross(j);
        }
        self.impulse += j_sum;
        self.ang_impulse += torque;
    }

    /// body_contact_friction 0x40d560.
    fn contact_friction(&self, j: &mut Vec3, c: &BodyContact, vel: Vec3) {
        let jn = j.dot(c.n);
        if jn < 0.0 {
            return;
        }
        let vt = vel - c.n * vel.dot(c.n);
        let k = self.setup.grip * jn * K120;
        let mut f = vt * -k;
        let mut mag = k * vt.length();
        let cap = self.setup.gravity * self.setup.mass * DT * 0.5;
        if mag > cap {
            f *= cap / mag;
            mag = cap;
        }
        if mag > c.mu_s * jn && mag > 0.0 {
            f *= c.mu_k * jn / mag;
        }
        *j += f;
    }

    /// set_car_ang_resistance 0x418b30: less rotational damping in the air.
    fn set_ang_resistance(&mut self) {
        let grounded = (0..4).any(|n| self.wheels[n].flags & W_CONTACT != 0);
        self.ang_res_current = if grounded { self.setup.ang_res } else { self.setup.ang_res * self.setup.res_mod };
    }
}

/// Car-versus-car contact between the two cars' hull spheres: resolves the
/// deepest overlap with a restitution impulse and splits the overlap. The
/// original resolves car pairs with convex hulls (body_body_coll_dispatch
/// 0x40dc20), which is not ported.
pub fn collide_cars(a: &mut Car, b: &mut Car) {
    if a.pos.distance_squared(b.pos) > 400.0 * 400.0 {
        return;
    }
    let mut deepest: Option<(Vec3, f32, Vec3)> = None; // normal a→b, depth, contact point
    for &(la, ra) in &a.setup.hull {
        let ca = a.pos + a.rot * la;
        for &(lb, rb) in &b.setup.hull {
            let d = (b.pos + b.rot * lb) - ca;
            let dist = d.length();
            let depth = ra + rb - dist;
            if depth > 0.0 && dist > 1e-3 && deepest.is_none_or(|(_, best, _)| depth > best) {
                let n = d / dist;
                deepest = Some((n, depth, ca + n * ra));
            }
        }
    }
    let Some((n, depth, point)) = deepest else { return };
    let (ma, mb) = (a.setup.inv_mass, b.setup.inv_mass);
    a.pos -= n * depth * ma / (ma + mb);
    b.pos += n * depth * mb / (ma + mb);
    let (ra, rb) = (point - a.pos, point - b.pos);
    let vrel = (b.vel + b.ang_vel.cross(rb)) - (a.vel + a.ang_vel.cross(ra));
    let vn = vrel.dot(n);
    if vn >= 0.0 {
        return;
    }
    let k = ma + mb
        + n.dot((a.inv_inertia_world * ra.cross(n)).cross(ra))
        + n.dot((b.inv_inertia_world * rb.cross(n)).cross(rb));
    let j = n * (-(1.0 + 0.3) * vn / k);
    a.vel -= j * ma;
    a.ang_vel -= a.inv_inertia_world * ra.cross(j);
    b.vel += j * mb;
    b.ang_vel += b.inv_inertia_world * rb.cross(j);
    let hit = j.length();
    a.impact = a.impact.max(hit * ma);
    b.impact = b.impact.max(hit * mb);
}

/// spring_force 0x49b450: damping may never push the wheel further from rest.
fn spring_force(w: &WheelSetup, x: f32, v: f32) -> f32 {
    let f = -x * w.stiffness - w.damping * v;
    if sign(f) == sign(x) { 0.0 } else { f }
}

fn sign(v: f32) -> i32 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}

fn approach(value: f32, target: f32, rate: f32) -> f32 {
    if (target - value).abs() <= rate {
        target
    } else {
        value + rate * (target - value).signum()
    }
}

fn snap(v: f32) -> f32 {
    if v.abs() < 0.005 { 0.0 } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel(stiffness: f32, damping: f32) -> WheelSetup {
        WheelSetup { stiffness, damping, ..Default::default() }
    }

    #[test]
    fn spring_never_pushes_away_from_rest() {
        let w = wheel(300.0, 7.0);
        // Displacement either way is restored.
        assert!(spring_force(&w, -2.0, 0.0) > 0.0);
        assert!(spring_force(&w, 2.0, 0.0) < 0.0);
        // But damping can't make it push further from rest.
        assert_eq!(spring_force(&w, -1.0, 100.0), 0.0);
        assert_eq!(spring_force(&w, 1.0, -100.0), 0.0);
    }

    #[test]
    fn steering_rate_follows_car_control_input() {
        let setup = CarSetup::new(&CarParams { steer_rate: 3.0, engine_rate: 4.5, ..Default::default() }, None, Vec3::splat(-10.0), Vec3::splat(10.0));
        let mut car = Car::new(setup, Vec3::ZERO, Quat::IDENTITY);
        // From centre, turning in moves at 4× the base rate for one step...
        car.control(DriveInput { throttle: 0.0, steer: 1.0 });
        assert!((car.steer - 3.0 * DT * 4.0).abs() < 1e-6, "{}", car.steer);
        // ...then at half rate while continuing in the same direction.
        let before = car.steer;
        car.control(DriveInput { throttle: 0.0, steer: 1.0 });
        assert!((car.steer - before - 3.0 * DT * 0.5).abs() < 1e-6);
        // Throttle snaps to zero when reversed, then ramps at EngineRate.
        car.throttle = 0.5;
        car.control(DriveInput { throttle: -1.0, steer: 0.0 });
        assert!((car.throttle + 4.5 * DT).abs() < 1e-6, "{}", car.throttle);
    }

    #[test]
    fn material_table_matches_the_game() {
        assert_eq!(materials::MATERIALS.len(), 27);
        assert_eq!(materials::get(10).roughness, 0.4);
        assert_eq!(materials::get(99).roughness, 1.0); // out of range falls back to default
        assert!(materials::get(16).surface_velocity().length() > 400.0);
    }
}
