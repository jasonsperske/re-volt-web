//! Pickups and weapons, ported from REVOLT.EXE as documented in
//! docs/weapons.md (section numbers below refer to it). Game space: Y down.
//!
//! Stand-ins where the original wasn't decompiled or was simplified:
//! - firework target choice (sub_49ae90): nearest opponent in a forward cone;
//! - when CPU cars fire: an original heuristic (`ai_should_fire`);
//! - projectiles are spheres against track faces (no edges), and the
//!   firework's spin is reduced to steering its nose;
//! - the putty bomb's blast sums one ground ray, not every polygon below;
//! - fireworks explode on hitting a wall (as the game's README describes);
//! - visual shells (electro pulse, turbo) and particles are not drawn.

use glam::{Quat, Vec3};

use crate::car::{Car, OilZone, DT};
use crate::collision::{CollisionWorld, NCP_OBJECTS_IGNORE};
use crate::formats::fob::LevelObject;

/// `.fob` object types (§1.2).
pub const PICKUP_KIND: i32 = 30;
pub const STAR_KIND: i32 = 50;

const GRAVITY: f32 = 2200.0;
const ROULETTE_TIME: f32 = 4.0;
const PICKUP_DELAY: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weapon {
    Shockwave,
    Firework,
    FireworkPack,
    Bomb,
    WaterBalloons,
    ElectroPulse,
    OilSlick,
    BallBearing,
    Turbo,
    Clone,
    GlobalPulse,
}

impl Weapon {
    pub const ALL: [Weapon; 11] = [
        Weapon::Shockwave,
        Weapon::Firework,
        Weapon::FireworkPack,
        Weapon::Bomb,
        Weapon::WaterBalloons,
        Weapon::ElectroPulse,
        Weapon::OilSlick,
        Weapon::BallBearing,
        Weapon::Turbo,
        Weapon::Clone,
        Weapon::GlobalPulse,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Weapon::Shockwave => "Shockwave",
            Weapon::Firework => "Firework",
            Weapon::FireworkPack => "Firework Pack",
            Weapon::Bomb => "Bomb",
            Weapon::WaterBalloons => "Water Balloons",
            Weapon::ElectroPulse => "Electro Pulse",
            Weapon::OilSlick => "Oil Slick",
            Weapon::BallBearing => "Ball Bearing",
            Weapon::Turbo => "Turbo Battery",
            Weapon::Clone => "Clone Pickup",
            Weapon::GlobalPulse => "Global Pulse",
        }
    }

    /// Uses per pickup (give_weapon 0x47c120).
    fn uses(self) -> u32 {
        match self {
            Weapon::FireworkPack | Weapon::WaterBalloons => 3,
            _ => 1,
        }
    }
}

/// Weapon weights by race position (§3.2, 0x4c0f64): leader, middle, last
/// place, and CPU cars.
const WEIGHTS: [[u32; 11]; 4] = [
    [5, 18, 5, 10, 5, 18, 18, 4, 8, 9, 0],
    [10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 0],
    [20, 3, 15, 2, 15, 2, 2, 21, 13, 2, 5],
    [10, 10, 5, 10, 10, 5, 15, 15, 10, 10, 0],
];

/// normalize_weapon_weight_tables 0x47b770: cumulative, scaled to 32768.
fn cumulative(w: &[u32; 11]) -> [u32; 11] {
    let total: u32 = w.iter().sum();
    let mut out = [0; 11];
    let mut acc = 0;
    for i in 0..11 {
        acc += (w[i] << 15) / total.max(1);
        out[i] = acc;
    }
    out
}

/// Cumulative weapon table per race position plus one for CPU cars
/// (build_position_weapon_tables 0x47b7c0, including its midpoint jump).
fn position_tables(cars: usize) -> (Vec<[u32; 11]>, [u32; 11]) {
    let rows = WEIGHTS.map(|w| cumulative(&w));
    let lerp = |a: &[u32; 11], b: &[u32; 11], t: u32| -> [u32; 11] {
        std::array::from_fn(|i| ((a[i] as u64 * (32768 - t as u64) + b[i] as u64 * t as u64) >> 15) as u32)
    };
    let fix = |mut r: [u32; 11]| {
        if r[10] != 0x7fff {
            if r[10] == r[9] {
                r[9] = 0x7fff;
            }
            r[10] = 0x7fff;
        }
        r
    };
    let n = cars.max(2);
    let tables = (0..n)
        .map(|p| {
            let t = (p as u32 * 32768) / (n as u32 - 1);
            fix(if p < n / 2 { lerp(&rows[0], &rows[1], t) } else { lerp(&rows[1], &rows[2], t) })
        })
        .collect();
    (tables, fix(rows[3]))
}

/// MSVC `rand()`, as used throughout the game: 0..32767.
struct Rand(u32);

impl Rand {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(214013).wrapping_add(2531011);
        (self.0 >> 16) & 0x7fff
    }

    fn unit(&mut self) -> f32 {
        self.next() as f32 / 32768.0
    }

    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PickupState {
    /// Appears when the timer runs out.
    Waiting(f32),
    /// Spinning on the track; the value is time since appearing.
    Live(f32),
    /// Flying away after collection.
    Collected(f32),
    Dormant,
    Gone,
}

struct Pickup {
    home: Vec3,
    pos: Vec3,
    state: PickupState,
    clone: bool,
    vel: Vec3,
}

/// Per-racer weapon state.
#[derive(Clone, Debug)]
pub struct Armory {
    pub weapon: Option<Weapon>,
    pub uses: u32,
    roulette: f32,
    chosen: usize,
    /// Electro zap timer: >0 zapped, 0..-3 immune, -3 recovered (§4.5).
    pub zap: f32,
    bomb_cooldown: f32,
    turbo: Option<f32>,
    held: f32,
}

impl Default for Armory {
    fn default() -> Self {
        Armory { weapon: None, uses: 0, roulette: 0.0, chosen: 0, zap: -3.0, bomb_cooldown: 0.0, turbo: None, held: 0.0 }
    }
}

impl Armory {
    /// Weapon shown by the spinning roulette, if it's running (§3.4).
    pub fn roulette_display(&self) -> Option<Weapon> {
        (self.roulette > 0.0).then(|| {
            let t = self.roulette;
            let slot = (self.chosen as f32 + 22.0 - 0.34375 * t * t * t).rem_euclid(11.0);
            Weapon::ALL[(slot as usize).min(10)]
        })
    }
}

/// A simple rigid sphere for projectiles, integrated like the game's
/// particles (impulse, `1 - 120·dt·Resistance` damping, semi-implicit Euler).
struct Ball {
    pos: Vec3,
    vel: Vec3,
    radius: f32,
    mass: f32,
    hardness: f32,
    resistance: f32,
}

impl Ball {
    /// One fixed step with gravity and (optionally) track collision; returns
    /// the strongest impact (normal, closing speed).
    fn step(&mut self, world: &CollisionWorld, extra_impulse: Vec3, gravity: bool) -> Option<(Vec3, f32)> {
        self.step_with(world, extra_impulse, gravity, true)
    }

    fn step_with(&mut self, world: &CollisionWorld, extra_impulse: Vec3, gravity: bool, collide: bool) -> Option<(Vec3, f32)> {
        let mut j = extra_impulse;
        if gravity {
            j += Vec3::Y * (self.mass * GRAVITY * DT);
        }
        self.vel += j / self.mass;
        self.vel *= 1.0 - 120.0 * DT * self.resistance;
        self.pos += self.vel * DT;
        if !collide {
            return None;
        }
        let mut contacts = Vec::new();
        world.sphere_contacts(self.pos, self.radius, NCP_OBJECTS_IGNORE, &mut contacts);
        let mut hit: Option<(Vec3, f32)> = None;
        for c in &contacts {
            self.pos += c.normal * c.depth;
            let vn = self.vel.dot(c.normal);
            if vn < 0.0 {
                self.vel -= c.normal * vn * (1.0 + self.hardness);
                if hit.is_none_or(|(_, s)| -vn > s) {
                    hit = Some((c.normal, -vn));
                }
            }
        }
        hit
    }
}

enum Kind {
    Shockwave { age: f32, prev: Vec3 },
    Firework { age: f32, target: Option<usize>, nose: Vec3, spin: Vec3 },
    WaterBalloon { age: f32, energy: f32 },
    ChromeBall { age: f32 },
}

struct Projectile {
    owner: usize,
    ball: Ball,
    kind: Kind,
    dead: bool,
}

struct Bomb {
    carrier: usize,
    fuse: f32,
    lit: bool,
}

struct OilBlob {
    x: f32,
    z: f32,
    ground: f32,
    radius: f32,
    target: f32,
    age: f32,
}

struct OilDropper {
    owner: usize,
    last: Vec3,
    count: u32,
    age: f32,
}

struct Pulse {
    owner: usize,
    age: f32,
}

/// Something to draw this frame (game space).
pub struct Visual {
    /// Model stem in MODELS/ ("PICKUP", ...) or "OIL" for an oil disc.
    pub model: &'static str,
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: f32,
}

/// A sound to play, at a game-space position.
pub struct Sound {
    pub path: &'static str,
    pub pos: Vec3,
}

pub struct Weapons {
    pickups: Vec<Pickup>,
    stars: Vec<(Vec3, bool)>,
    pub armory: Vec<Armory>,
    projectiles: Vec<Projectile>,
    bombs: Vec<Bomb>,
    blobs: Vec<OilBlob>,
    droppers: Vec<OilDropper>,
    pulses: Vec<Pulse>,
    tables: Vec<[u32; 11]>,
    cpu_table: [u32; 11],
    rng: Rand,
    time: f32,
    accumulator: f32,
    pub sounds: Vec<Sound>,
}

/// Collection box around a pickup (§2): x ±30, y −25/+55, z ±30.
fn pickup_box(p: Vec3) -> (Vec3, Vec3) {
    (p - Vec3::new(30.0, 25.0, 30.0), p + Vec3::new(30.0, 55.0, 30.0))
}

fn overlaps(a: (Vec3, Vec3), b: (Vec3, Vec3)) -> bool {
    a.0.cmple(b.1).all() && b.0.cmple(a.1).all()
}

fn line_of_sight(world: &CollisionWorld, a: Vec3, b: Vec3) -> bool {
    let d = b - a;
    let len = d.length();
    len < 1.0 || world.raycast(a, d / len, len, NCP_OBJECTS_IGNORE).is_none()
}

impl Weapons {
    pub fn new(objects: &[LevelObject], racers: usize, seed: u32) -> Self {
        let pickups = objects
            .iter()
            .filter(|o| o.kind == PICKUP_KIND && o.flags[0] == 0)
            .map(|o| Pickup { home: Vec3::from(o.pos), pos: Vec3::from(o.pos), state: PickupState::Dormant, clone: false, vel: Vec3::ZERO })
            .collect();
        let stars = objects.iter().filter(|o| o.kind == STAR_KIND && o.flags[0] == 0).map(|o| (Vec3::from(o.pos), true)).collect();
        let (tables, cpu_table) = position_tables(racers);
        let mut w = Weapons {
            pickups,
            stars,
            armory: vec![Armory::default(); racers],
            projectiles: Vec::new(),
            bombs: Vec::new(),
            blobs: Vec::new(),
            droppers: Vec::new(),
            pulses: Vec::new(),
            tables,
            cpu_table,
            rng: Rand(seed),
            time: 0.0,
            accumulator: 0.0,
            sounds: Vec::new(),
        };
        // spawn_initial_pickups 0x47ba10: 2N+5 random points, live after 5 s.
        let live = (2 * racers + 5).min(w.pickups.len());
        for _ in 0..live {
            w.activate_random_pickup();
        }
        w
    }

    /// activate_random_pickup 0x47ba50: wake a random dormant point.
    fn activate_random_pickup(&mut self) -> bool {
        let dormant: Vec<usize> =
            (0..self.pickups.len()).filter(|&i| self.pickups[i].state == PickupState::Dormant && !self.pickups[i].clone).collect();
        if dormant.is_empty() {
            return false;
        }
        let i = dormant[self.rng.next() as usize % dormant.len()];
        let p = &mut self.pickups[i];
        p.state = PickupState::Waiting(PICKUP_DELAY);
        p.pos = p.home;
        true
    }

    /// Advances everything by `dt`. `positions[i]` is racer i's race position
    /// (0 = leader); `cpu[i]` marks AI cars; `fire[i]` is a newly pressed fire.
    pub fn update(
        &mut self,
        world: &CollisionWorld,
        cars: &mut [&mut Car],
        positions: &[usize],
        cpu: &[bool],
        fire: &[bool],
        dt: f32,
    ) {
        self.sounds.clear();
        self.time += dt;
        let n = cars.len().min(self.armory.len());

        self.update_pickups(cars, positions, cpu, dt);

        // Roulette, zap, turbo and firing.
        for i in 0..n {
            let a = &mut self.armory[i];
            if a.roulette > 0.0 {
                a.roulette = (a.roulette - dt).max(0.0);
                if a.roulette == 0.0 {
                    let w = Weapon::ALL[a.chosen.min(10)];
                    self.give(i, w, cars);
                }
            }
            let a = &mut self.armory[i];
            if a.zap > -3.0 {
                a.zap = (a.zap - dt).max(-3.0);
            }
            a.bomb_cooldown = (a.bomb_cooldown - dt).max(0.0);
            cars[i].zapped = a.zap > 0.0;
            if let Some(t) = &mut a.turbo {
                *t += dt;
                if *t < 10.0 {
                    cars[i].top_speed_scale = 2.0 - *t / 10.0;
                    cars[i].friction_override = Some(2.0);
                    cars[i].down_force_override = Some(4.0);
                } else {
                    a.turbo = None;
                    cars[i].top_speed_scale = 1.0;
                    cars[i].friction_override = None;
                    cars[i].down_force_override = None;
                }
            }
            if a.uses > 0 {
                a.held += dt;
                let wants = if cpu[i] { self.ai_should_fire(i, cars) } else { fire.get(i).copied().unwrap_or(false) };
                if wants {
                    self.fire(i, world, cars);
                }
            }
        }

        // Projectiles step on the physics clock.
        self.accumulator = (self.accumulator + dt).min(0.1);
        while self.accumulator >= DT {
            self.accumulator -= DT;
            self.step_projectiles(world, cars);
        }
        self.projectiles.retain(|p| !p.dead);

        self.update_bombs(world, cars, dt);
        self.update_oil(world, cars, dt);
        self.update_pulses(world, cars, dt);
    }

    fn update_pickups(&mut self, cars: &mut [&mut Car], positions: &[usize], cpu: &[bool], dt: f32) {
        let boxes: Vec<(Vec3, Vec3)> = cars.iter().map(|c| c.aabb()).collect();
        for k in 0..self.pickups.len() {
            match self.pickups[k].state {
                PickupState::Waiting(t) => {
                    if t - dt <= 0.0 {
                        self.pickups[k].state = PickupState::Live(0.0);
                        self.sounds.push(Sound { path: "WAVS/PICKGEN.WAV", pos: self.pickups[k].pos });
                    } else {
                        self.pickups[k].state = PickupState::Waiting(t - dt);
                    }
                }
                PickupState::Live(t) => {
                    self.pickups[k].state = PickupState::Live(t + dt);
                    let bx = pickup_box(self.pickups[k].pos);
                    let clone = self.pickups[k].clone;
                    for i in 0..cars.len().min(self.armory.len()) {
                        let a = &self.armory[i];
                        let eligible = (a.uses == 0 && a.roulette == 0.0) || clone;
                        if eligible && overlaps(bx, boxes[i]) {
                            let v = cars[i].vel;
                            self.pickups[k].state = PickupState::Collected(0.0);
                            self.pickups[k].vel = Vec3::new(0.5 * v.x, -64.0, 0.5 * v.z);
                            if clone {
                                // A clone hands over a lit bomb that goes off at once.
                                self.sounds.push(Sound { path: "WAVS/PUTTBANG.WAV", pos: cars[i].pos });
                                self.bombs.push(Bomb { carrier: i, fuse: 0.0, lit: true });
                            } else {
                                self.sounds.push(Sound { path: "WAVS/PICKUP.WAV", pos: cars[i].pos });
                                self.start_roulette(i, positions.get(i).copied().unwrap_or(0), cpu[i]);
                            }
                            break;
                        }
                    }
                }
                PickupState::Collected(t) => {
                    let t = t + dt;
                    let p = &mut self.pickups[k];
                    p.pos = p.home + p.vel * t;
                    if t > 1.0 {
                        if p.clone {
                            p.state = PickupState::Gone;
                        } else {
                            p.state = PickupState::Dormant;
                            if !self.activate_random_pickup() {
                                // No spare point: this one reappears.
                                let p = &mut self.pickups[k];
                                p.state = PickupState::Waiting(PICKUP_DELAY);
                                p.pos = p.home;
                            }
                        }
                    } else {
                        p.state = PickupState::Collected(t);
                    }
                }
                PickupState::Dormant | PickupState::Gone => {}
            }
        }
        self.pickups.retain(|p| p.state != PickupState::Gone);

        // Electro stars give the Global Pulse directly (§3.6). They stay
        // collected for the rest of the race (inferred).
        for s in 0..self.stars.len() {
            if !self.stars[s].1 {
                continue;
            }
            let bx = pickup_box(self.stars[s].0);
            for i in 0..cars.len().min(self.armory.len()) {
                if self.armory[i].uses == 0 && self.armory[i].roulette == 0.0 && overlaps(bx, boxes[i]) {
                    self.stars[s].1 = false;
                    self.sounds.push(Sound { path: "WAVS/PICKUP.WAV", pos: cars[i].pos });
                    self.give(i, Weapon::GlobalPulse, cars);
                    break;
                }
            }
        }
    }

    /// Chooses the weapon now and spins the roulette for 4 s (§3.3).
    fn start_roulette(&mut self, i: usize, position: usize, cpu: bool) {
        let row = if cpu { self.cpu_table } else { self.tables[position.min(self.tables.len() - 1)] };
        let r = self.rng.next();
        let mut prev = 0;
        let mut w = 0;
        while w < 10 {
            let c = row[w];
            if c != prev && r < c {
                break;
            }
            prev = c;
            w += 1;
        }
        let a = &mut self.armory[i];
        a.chosen = w;
        a.roulette = ROULETTE_TIME;
    }

    fn give(&mut self, i: usize, w: Weapon, cars: &mut [&mut Car]) {
        if w == Weapon::Bomb {
            // The bomb goes live the moment it's awarded (give_weapon).
            self.arm_bomb(i, cars[i].pos);
            return;
        }
        let a = &mut self.armory[i];
        a.weapon = Some(w);
        a.uses = w.uses();
        a.held = 0.0;
    }

    fn arm_bomb(&mut self, i: usize, pos: Vec3) {
        if self.bombs.iter().any(|b| b.carrier == i) {
            return;
        }
        self.bombs.push(Bomb { carrier: i, fuse: 10.0, lit: false });
        self.armory[i].bomb_cooldown = 3.0;
        self.sounds.push(Sound { path: "WAVS/FUSE.WAV", pos });
    }

    /// Fires one use (fire_weapon 0x41ed80).
    fn fire(&mut self, i: usize, world: &CollisionWorld, cars: &mut [&mut Car]) {
        let Some(w) = self.armory[i].weapon else { return };
        let car = &*cars[i];
        let (pos, rot, vel) = (car.pos, car.rot, car.vel);
        let look = rot * Vec3::Z;
        let up = rot * Vec3::Y; // the car's floor-down axis
        let launch = pos + rot * car.setup.weapon_offset;
        match w {
            Weapon::Shockwave => {
                self.projectiles.push(Projectile {
                    owner: i,
                    ball: Ball { pos: launch, vel: look * 6000.0, radius: 40.0, mass: 1.0, hardness: 0.1, resistance: 0.0 },
                    kind: Kind::Shockwave { age: 0.0, prev: launch },
                    dead: false,
                });
                self.sounds.push(Sound { path: "WAVS/SHOCKFIRE.WAV", pos });
            }
            Weapon::Firework | Weapon::FireworkPack => {
                let start = launch - Vec3::Y * 20.0; // raised to roof height
                let target = self.pick_target(i, cars);
                // The rocket's local +Y points backwards; the nose is -row1.
                let (row1, q) = match target {
                    Some(t) => {
                        let d = cars[t].pos - start;
                        let l = d.length() / 2.0;
                        let tq = (l - 100.0) / 5900.0;
                        ((-d + up * (l / 6000.0)).normalize_or(-look), 0.125 * tq.powi(4))
                    }
                    None => ((-0.6667 * look + 0.3333 * up).normalize(), 0.25),
                };
                let dud = !line_of_sight(world, pos, start + look * 35.0);
                self.projectiles.push(Projectile {
                    owner: i,
                    ball: Ball { pos: start, vel: vel + row1 * (1500.0 * q), radius: 10.0, mass: 0.1, hardness: 0.7, resistance: 0.003 },
                    kind: Kind::Firework { age: if dud { 1.2 } else { 0.0 }, target, nose: -row1, spin: Vec3::ZERO },
                    dead: false,
                });
                self.sounds.push(Sound { path: "WAVS/FIREFIRE.WAV", pos });
            }
            Weapon::Bomb => self.arm_bomb(i, pos),
            Weapon::WaterBalloons => {
                let start = pos + rot * Vec3::new(0.0, -16.0, 80.0);
                let energy = 500.0 + self.rng.next() as f32 * 0.061;
                self.projectiles.push(Projectile {
                    owner: i,
                    ball: Ball { pos: start, vel: vel + rot * Vec3::new(0.0, -500.0, 4000.0), radius: 45.0, mass: 0.6, hardness: 0.2, resistance: 0.001 },
                    kind: Kind::WaterBalloon { age: 0.0, energy },
                    dead: false,
                });
                self.sounds.push(Sound { path: "WAVS/WBOMBFIRE.WAV", pos });
            }
            Weapon::ElectroPulse => {
                self.pulses.push(Pulse { owner: i, age: 0.0 });
                self.sounds.push(Sound { path: "WAVS/ELECTRO.WAV", pos });
            }
            Weapon::OilSlick => {
                self.droppers.push(OilDropper { owner: i, last: pos, count: 0, age: 0.0 });
                self.sounds.push(Sound { path: "WAVS/OILDROP.WAV", pos });
            }
            Weapon::BallBearing => {
                let start = pos + look * (-30.0 * 1.5 - 64.0);
                let start = if line_of_sight(world, pos, start) { start } else { pos - Vec3::Y * 75.0 };
                self.projectiles.push(Projectile {
                    owner: i,
                    ball: Ball { pos: start, vel: vel - look * 512.0, radius: 4.0, mass: 3.0, hardness: 0.2, resistance: 0.001 },
                    kind: Kind::ChromeBall { age: 0.0 },
                    dead: false,
                });
                self.sounds.push(Sound { path: "WAVS/BALLDROP.WAV", pos });
            }
            Weapon::Turbo => {
                let a = &mut self.armory[i];
                if a.zap > 0.0 {
                    a.zap = 0.0; // using it while zapped just cures the zap
                    self.sounds.push(Sound { path: "WAVS/SHOCK.WAV", pos });
                } else {
                    a.turbo = Some(0.0);
                    self.sounds.push(Sound { path: "WAVS/TURBO.WAV", pos });
                }
            }
            Weapon::Clone => {
                // Dropped behind the car, a little up; live at once.
                let home = pos + look * (-30.0 - 128.0) - Vec3::Y * 40.0;
                self.pickups.push(Pickup { home, pos: home, state: PickupState::Live(1.6), clone: true, vel: Vec3::ZERO });
            }
            Weapon::GlobalPulse => {
                for (k, a) in self.armory.iter_mut().enumerate() {
                    if k != i {
                        a.zap = 3.0; // ignores immunity
                    }
                }
                self.sounds.push(Sound { path: "WAVS/STARFIRE.WAV", pos });
            }
        }
        let a = &mut self.armory[i];
        a.uses = a.uses.saturating_sub(1);
        a.held = 0.0;
        if a.uses == 0 {
            a.weapon = None;
        }
    }

    /// Stand-in for the game's target choice: nearest opponent within a
    /// 60° forward cone and 6000 units.
    fn pick_target(&self, i: usize, cars: &[&mut Car]) -> Option<usize> {
        let (pos, look) = (cars[i].pos, cars[i].forward());
        (0..cars.len())
            .filter(|&k| k != i)
            .filter(|&k| {
                let d = cars[k].pos - pos;
                d.length() < 6000.0 && d.normalize_or_zero().dot(look) > 0.5
            })
            .min_by(|&a, &b| cars[a].pos.distance(pos).total_cmp(&cars[b].pos.distance(pos)))
    }

    /// When CPU cars use their weapon (original heuristic, not ported).
    fn ai_should_fire(&self, i: usize, cars: &[&mut Car]) -> bool {
        let a = &self.armory[i];
        let Some(w) = a.weapon else { return false };
        if a.held > 8.0 {
            return true;
        }
        let (pos, look) = (cars[i].pos, cars[i].forward());
        let near = |ahead: bool, range: f32, cone: f32| {
            (0..cars.len()).any(|k| {
                if k == i {
                    return false;
                }
                let d = cars[k].pos - pos;
                let along = d.normalize_or_zero().dot(look);
                d.length() < range && if ahead { along > cone } else { along < -cone }
            })
        };
        match w {
            Weapon::Shockwave | Weapon::Firework | Weapon::FireworkPack | Weapon::WaterBalloons => {
                a.held > 0.3 && near(true, 2500.0, 0.85)
            }
            Weapon::ElectroPulse => near(true, 600.0, -1.0),
            Weapon::OilSlick | Weapon::BallBearing | Weapon::Clone => near(false, 1500.0, 0.7),
            Weapon::Turbo => a.held > 1.0,
            Weapon::GlobalPulse => true,
            Weapon::Bomb => false,
        }
    }

    fn step_projectiles(&mut self, world: &CollisionWorld, cars: &mut [&mut Car]) {
        let mut blasts: Vec<(Vec3, Blast)> = Vec::new();
        for p in &mut self.projectiles {
            match &mut p.kind {
                Kind::Shockwave { age, prev } => {
                    p.ball.step(world, Vec3::ZERO, true);
                    // §4.1: lift and roll cars inside a flattened ellipsoid.
                    for (k, car) in cars.iter_mut().enumerate() {
                        if k == p.owner {
                            continue;
                        }
                        let d = car.pos - p.ball.pos;
                        let s = d.x * d.x + (10.0 * d.y).powi(2) + d.z * d.z;
                        if s <= 1e6 {
                            let f = 1.0 - s * 1e-6;
                            let (mass, ixx, fwd) = (car.setup.mass, car.inertia_xx(), car.forward());
                            car.add_impulse(Vec3::Y * (-f * mass * DT * 4500.0));
                            car.add_angular_impulse(fwd * (f * ixx * DT * 100.0));
                        }
                    }
                    *age += DT;
                    if *age > 0.25 && (p.ball.pos - *prev).length() / DT < 1500.0 {
                        *age = 2.0; // stalled against something
                    }
                    *prev = p.ball.pos;
                    if *age >= 2.0 {
                        p.dead = true;
                    }
                }
                Kind::Firework { age, target, nose, spin } => {
                    *age += DT;
                    if *age >= 1.2 {
                        blasts.push((p.ball.pos, Blast::Firework));
                        p.dead = true;
                        continue;
                    }
                    let row1 = -*nose;
                    let mut j = Vec3::ZERO;
                    match target.and_then(|t| cars.get(t)) {
                        Some(t) => {
                            let d = t.pos - p.ball.pos;
                            let dist = d.length();
                            if dist < 1e-5 {
                                *age = 1.2;
                                continue;
                            }
                            let u = d / dist;
                            j += row1 * ((dist / 6000.0) * GRAVITY * DT * -0.5);
                            j += u * (0.5 * GRAVITY * DT);
                            // Steer the nose: angular impulse 50·(u × row1), I = 100.
                            *spin += u.cross(row1) * (50.0 * 0.01);
                        }
                        None => j += row1 * (-0.5 * GRAVITY * DT),
                    }
                    *spin *= 1.0 - 120.0 * DT * 0.01;
                    let turn = *spin * DT;
                    if turn.length() > 1e-6 {
                        *nose = (Quat::from_scaled_axis(turn) * *nose).normalize();
                    }
                    // firework_fly_homing never calls world collision; the
                    // unguided firework_fly does, and explodes on a wall.
                    let homing = target.is_some();
                    if let Some((_, speed)) = p.ball.step_with(world, j, true, !homing) {
                        if speed > 200.0 {
                            *age = 1.2;
                        }
                    }
                    let pos = p.ball.pos;
                    if cars.iter().enumerate().any(|(k, c)| k != p.owner && c.pos.distance_squared(pos) < 10000.0) {
                        *age = 1.2;
                    }
                }
                Kind::WaterBalloon { age, energy } => {
                    *age += DT;
                    if let Some((normal, impact)) = p.ball.step(world, Vec3::ZERO, true) {
                        *energy -= impact;
                        if normal.y > -0.7 && *energy < 0.0 {
                            *energy = 0.0; // walls don't pop it
                        }
                    }
                    let pos = p.ball.pos;
                    let hit_car = cars.iter().enumerate().any(|(k, c)| {
                        (k != p.owner || *age > 0.5) && c.world_spheres().any(|(s, r)| s.distance(pos) < r + p.ball.radius)
                    });
                    if hit_car || *energy < 0.0 || *age > 30.0 {
                        blasts.push((pos, Blast::Water));
                        p.dead = true;
                    }
                }
                Kind::ChromeBall { age } => {
                    *age += DT;
                    p.ball.radius = if *age < 0.5 { 4.0 + 120.0 * *age } else if *age < 29.0 { 64.0 } else { 4.0 + 60.0 * (30.0 - *age) };
                    p.ball.step(world, Vec3::ZERO, true);
                    for car in cars.iter_mut() {
                        ball_hits_car(&mut p.ball, car);
                    }
                    if *age >= 30.0 {
                        p.dead = true;
                    }
                }
            }
        }
        for (pos, blast) in blasts {
            self.blast(pos, blast, cars);
        }
    }

    fn blast(&mut self, pos: Vec3, kind: Blast, cars: &mut [&mut Car]) {
        // §4.2 firework_explode and §4.4 water balloon burst.
        let (radius2, min2, lift, offset, sound) = match kind {
            Blast::Firework => (250000.0, 5625.0, 0.0, 50.0, "WAVS/FIREBANG.WAV"),
            Blast::Water => (490000.0, 2500.0, 800000.0, 25.0, "WAVS/WBOMBHIT.WAV"),
        };
        self.sounds.push(Sound { path: sound, pos });
        for car in cars.iter_mut() {
            let d = car.pos - pos;
            if d.length_squared() >= radius2 {
                continue;
            }
            let d2 = d.length_squared().max(min2);
            let up = match kind {
                Blast::Firework => -100000.0 / (4.0 * d2.sqrt()),
                Blast::Water => -lift / d2,
            };
            let j = Vec3::new(100000.0 * d.x / d2, up, 100000.0 * d.z / d2);
            let at = car.pos + Vec3::new(self.rng.signed(), self.rng.signed(), self.rng.signed()) * offset;
            car.add_impulse_at(at, j);
        }
    }

    fn update_bombs(&mut self, world: &CollisionWorld, cars: &mut [&mut Car], dt: f32) {
        let mut explode = Vec::new();
        for (b_index, b) in self.bombs.iter_mut().enumerate() {
            b.fuse -= dt;
            if !b.lit {
                if b.fuse < 0.0 {
                    b.lit = true;
                    b.fuse = 1.0;
                    continue;
                }
                // Pass it on to any car that touches the carrier (§4.3).
                let carrier = b.carrier;
                let target = (0..cars.len()).find(|&k| {
                    k != carrier
                        && self.armory[k].bomb_cooldown <= 0.0
                        && cars[carrier].world_spheres().any(|(a, ra)| cars[k].world_spheres().any(|(c, rc)| a.distance(c) < ra + rc))
                });
                if let Some(k) = target {
                    self.armory[carrier].bomb_cooldown = 3.0;
                    b.carrier = k;
                }
            } else if b.fuse <= 0.0 {
                explode.push(b_index);
            }
        }
        for &k in explode.iter().rev() {
            let b = self.bombs.remove(k);
            let car = &mut *cars[b.carrier];
            self.sounds.push(Sound { path: "WAVS/PUTTBANG.WAV", pos: car.pos });
            // Thrown off the ground below it; tumbles if airborne.
            let k_mass = car.setup.mass * 0.2 + 0.4;
            match world.raycast(car.pos, Vec3::Y, 128.0, NCP_OBJECTS_IGNORE) {
                Some(hit) => {
                    let j = hit.normal * ((128.0 - hit.t) * k_mass * 10.0);
                    let angle = self.rng.unit() * std::f32::consts::TAU;
                    let r = self.rng.unit() * 90.0;
                    car.add_impulse_at(car.pos + Vec3::new(angle.cos() * r, 0.0, angle.sin() * r), j);
                }
                None => {
                    let spin = Vec3::new(self.rng.signed(), self.rng.signed(), self.rng.signed()) * 50000.0;
                    car.add_angular_impulse(spin);
                }
            }
        }
    }

    fn update_oil(&mut self, world: &CollisionWorld, cars: &mut [&mut Car], dt: f32) {
        // Droppers lay up to three blobs 64 units apart (§4.6).
        let mut new_blobs = Vec::new();
        for d in &mut self.droppers {
            d.age += dt;
            let car = &*cars[d.owner];
            if d.count == 0 || car.pos.distance(d.last) >= 64.0 {
                if d.count > 0 {
                    d.last += (car.pos - d.last).normalize_or_zero() * 64.0;
                }
                let local_back = car.aabb().0.distance(car.aabb().1) * 0.3;
                let at = car.pos - car.forward() * local_back;
                new_blobs.push((at, 128.0 - 16.0 * d.count as f32));
                d.count += 1;
            }
        }
        self.droppers.retain(|d| d.age <= 1.0 && d.count < 3);
        for (at, target) in new_blobs {
            let ground = world.raycast(at - Vec3::Y * 50.0, Vec3::Y, 1000.0, NCP_OBJECTS_IGNORE).map_or(at.y, |h| h.point.y);
            self.blobs.push(OilBlob { x: at.x, z: at.z, ground, radius: 32.0, target, age: 0.0 });
        }
        let mut zones = Vec::new();
        for b in &mut self.blobs {
            b.age += dt;
            b.radius += (b.target - b.radius) * dt;
            if b.age < 28.75 {
                zones.push(OilZone { x: b.x, z: b.z, radius: b.radius, y_min: b.ground - 2.0, y_max: b.ground + 2.0 });
            }
        }
        self.blobs.retain(|b| b.age < 30.0);
        for car in cars.iter_mut() {
            car.set_oil_zones(&zones);
        }
    }

    fn update_pulses(&mut self, world: &CollisionWorld, cars: &mut [&mut Car], dt: f32) {
        for p in &mut self.pulses {
            p.age += dt;
            let owner = cars[p.owner].pos;
            for k in 0..cars.len() {
                if k == p.owner || self.armory[k].zap != -3.0 {
                    continue;
                }
                if owner.distance_squared(cars[k].pos) < 147456.0 && line_of_sight(world, owner, cars[k].pos) {
                    self.armory[k].zap = 3.0;
                    self.sounds.push(Sound { path: "WAVS/ELECTROZAP.WAV", pos: cars[k].pos });
                }
            }
        }
        self.pulses.retain(|p| p.age < 10.0);
    }

    /// Hands racer `i` a weapon directly (tests, debugging).
    pub fn grant(&mut self, i: usize, w: Weapon, cars: &mut [&mut Car]) {
        self.give(i, w, cars);
    }

    /// Number of pickups currently on the track.
    pub fn live_pickups(&self) -> usize {
        self.pickups.iter().filter(|p| matches!(p.state, PickupState::Live(_))).count()
    }

    /// Positions of live pickups (game space).
    pub fn pickup_positions(&self) -> Vec<Vec3> {
        self.pickups.iter().filter(|p| matches!(p.state, PickupState::Live(_))).map(|p| p.pos).collect()
    }

    /// Whether racer `i` is carrying a bomb, and its fuse.
    pub fn bomb_fuse(&self, i: usize) -> Option<f32> {
        self.bombs.iter().find(|b| b.carrier == i).map(|b| if b.lit { 0.0 } else { b.fuse })
    }

    pub fn has_pulse(&self, i: usize) -> bool {
        self.pulses.iter().any(|p| p.owner == i)
    }

    /// Everything to draw this frame.
    pub fn visuals(&self) -> Vec<Visual> {
        let mut out = Vec::new();
        let spin = Quat::from_rotation_y(self.time * 0.5);
        for p in &self.pickups {
            let scale = match p.state {
                PickupState::Live(t) => pop_scale(t),
                PickupState::Collected(t) => 2.0 * t + 1.0,
                _ => continue,
            };
            if scale > 0.0 {
                out.push(Visual { model: "PICKUP", pos: p.pos, rot: spin, scale });
            }
        }
        for &(pos, live) in &self.stars {
            if live {
                out.push(Visual { model: "STAR", pos, rot: spin, scale: 1.0 });
            }
        }
        for p in &self.projectiles {
            let v = match &p.kind {
                Kind::Shockwave { .. } => Visual { model: "SPHERE", pos: p.ball.pos, rot: Quat::IDENTITY, scale: 40.0 / 30.0 },
                Kind::Firework { nose, .. } => {
                    Visual { model: "FIREWORK", pos: p.ball.pos, rot: Quat::from_rotation_arc(Vec3::Y, -*nose), scale: 1.0 }
                }
                Kind::WaterBalloon { .. } => Visual { model: "WBOMB", pos: p.ball.pos, rot: Quat::IDENTITY, scale: 0.7 },
                Kind::ChromeBall { .. } => Visual { model: "BALL", pos: p.ball.pos, rot: Quat::IDENTITY, scale: p.ball.radius / 100.0 },
            };
            out.push(v);
        }
        for b in &self.blobs {
            out.push(Visual { model: "OIL", pos: Vec3::new(b.x, b.ground - 1.0, b.z), rot: Quat::IDENTITY, scale: b.radius });
        }
        out
    }
}

#[derive(Clone, Copy)]
enum Blast {
    Firework,
    Water,
}

/// Grow-in "pop" for a newly appeared pickup (update_pickup, §2).
fn pop_scale(t: f32) -> f32 {
    use std::f32::consts::TAU;
    if t >= 1.6 {
        1.0
    } else if t < 0.5 {
        0.0
    } else if t < 1.0 {
        2.0 * (t - 0.5) + 0.6667 * (TAU * (t - 0.5)).sin()
    } else if t < 1.35 {
        1.0 - 0.16667 * ((t - 1.0) * 8.97597).sin()
    } else {
        1.0 + 0.083333 * ((t - 1.35) * 12.5664).sin()
    }
}

/// Heavy ball against a car's hull spheres: equal and opposite impulse.
fn ball_hits_car(ball: &mut Ball, car: &mut Car) {
    let Some((c, r)) = car.world_spheres().find(|(c, r)| c.distance(ball.pos) < r + ball.radius) else { return };
    let d = ball.pos - c;
    let dist = d.length().max(1e-3);
    let n = d / dist;
    ball.pos += n * (r + ball.radius - dist);
    let vn = (ball.vel - car.vel).dot(n);
    if vn >= 0.0 {
        return;
    }
    let inv = 1.0 / ball.mass + 1.0 / car.setup.mass;
    let j = n * (-(1.0 + 0.2) * vn / inv);
    ball.vel += j / ball.mass;
    car.add_impulse_at(c + n * r, -j);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_tables_end_at_full_range() {
        let (rows, cpu) = position_tables(4);
        assert_eq!(rows.len(), 4);
        for r in rows.iter().chain([&cpu]) {
            assert_eq!(r[10], 0x7fff);
            assert!(r.windows(2).all(|w| w[0] <= w[1]), "not cumulative: {r:?}");
        }
    }

    #[test]
    fn leader_never_rolls_global_pulse_but_last_place_can() {
        let (rows, _) = position_tables(4);
        let mut w = Weapons {
            pickups: vec![],
            stars: vec![],
            armory: vec![Armory::default(); 4],
            projectiles: vec![],
            bombs: vec![],
            blobs: vec![],
            droppers: vec![],
            pulses: vec![],
            tables: rows,
            cpu_table: cumulative(&WEIGHTS[3]),
            rng: Rand(1),
            time: 0.0,
            accumulator: 0.0,
            sounds: vec![],
        };
        let mut counts = [[0u32; 11]; 2];
        for _ in 0..20000 {
            w.start_roulette(0, 0, false);
            counts[0][w.armory[0].chosen] += 1;
            w.start_roulette(0, 3, false);
            counts[1][w.armory[0].chosen] += 1;
        }
        assert_eq!(counts[0][10], 0, "leader got global pulses: {:?}", counts[0]);
        assert!(counts[1][10] > 0, "last place never got a global pulse: {:?}", counts[1]);
        // Last place favours ball bearings and shockwaves over oil (row 2).
        assert!(counts[1][7] > counts[1][6] * 3);
    }

    #[test]
    fn roulette_lands_on_the_chosen_weapon() {
        let mut a = Armory { chosen: 6, roulette: 4.0, ..Default::default() };
        assert!(a.roulette_display().is_some());
        a.roulette = 1e-6;
        assert_eq!(a.roulette_display(), Some(Weapon::OilSlick));
    }
}
