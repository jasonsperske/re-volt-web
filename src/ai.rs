//! Computer-controlled drivers that follow a level's AI racing line
//! (`.fan`) using the same car physics as the player.
//!
//! This is an original controller built on the game's data, not a port of
//! the game's AI code: pure-pursuit steering toward a point on the racing
//! line ahead, speed chosen from the sharpness of the upcoming bends, and
//! recovery when stuck or flipped (using Flip Car after 3 s, as the game's
//! AI does at 0x408140).

use glam::{Quat, Vec3};

use crate::car::{Car, DriveInput};
use crate::collision::CollisionWorld;
use crate::formats::fan::AiNodes;

/// How long a car is over on its side or roof before it flips itself.
const FLIP_AFTER: f32 = 3.0;
/// Flips in a row without getting going again before repositioning.
const MAX_FLIPS: u32 = 3;
/// Time without reaching a new racing-line node before repositioning.
const LOST_AFTER: f32 = 8.0;

pub struct AiDriver {
    /// Segment start: the car is between `node` and its next node.
    node: usize,
    /// Small shift of this driver's line across the track (±0.04), so cars
    /// don't drive in single file. Kept small: the racing line steers round
    /// obstacles inside the lane (e.g. the ramp on Museum 1).
    line_bias: f32,
    /// Fraction of each car's top speed this driver aims for.
    pace: f32,
    stuck_time: f32,
    reverse_time: f32,
    flipped_time: f32,
    /// Flips since the car last got going again.
    flips: u32,
    /// Time since the car last reached a new racing-line node.
    since_progress: f32,
    /// Heading turned through (radians) since the last node advance; a full
    /// circle means the car is orbiting a target it can't reach.
    turned: f32,
    last_heading: Option<Vec3>,
    seed: u32,
}

impl AiDriver {
    pub fn new(nodes: &AiNodes, pos: Vec3, seed: u32) -> Self {
        let mut d = AiDriver {
            node: nearest_node(nodes, pos),
            line_bias: 0.0,
            pace: 0.92,
            stuck_time: 0.0,
            reverse_time: 0.0,
            flipped_time: 0.0,
            flips: 0,
            since_progress: 0.0,
            turned: 0.0,
            last_heading: None,
            seed: seed.wrapping_mul(2654435761).max(1),
        };
        d.line_bias = (d.random() - 0.5) * 0.08;
        d.pace = 0.88 + d.random() * 0.1;
        d
    }

    /// Current racing-line node (for debugging).
    pub fn node(&self) -> usize {
        self.node
    }

    fn random(&mut self) -> f32 {
        // xorshift32: deterministic per driver.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 10000) as f32 / 10000.0
    }

    fn next_of(&self, nodes: &AiNodes, i: usize) -> usize {
        self.peek_next(nodes, i)
    }

    fn line_point(&self, nodes: &AiNodes, i: usize) -> Vec3 {
        let n = &nodes.nodes[i];
        let t = (n.racing_line + self.line_bias).clamp(0.12, 0.88);
        Vec3::from(n.left).lerp(Vec3::from(n.right), t)
    }

    /// Decides this frame's controls for `car`. Flips a car that's stuck on
    /// its side or roof, and repositions one that a flip can't help.
    pub fn drive(&mut self, nodes: &AiNodes, world: &CollisionWorld, car: &mut Car, dt: f32) -> DriveInput {
        if nodes.nodes.len() < 2 {
            return DriveInput::default();
        }
        let pos = car.pos;
        let before = self.node;
        self.advance(nodes, pos);
        let heading = car.forward();
        if self.node != before {
            self.turned = 0.0;
            self.since_progress = 0.0;
        } else if let Some(h) = self.last_heading {
            let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).normalize_or_zero();
            self.turned += flat(h).dot(flat(heading)).clamp(-1.0, 1.0).acos();
        }
        self.last_heading = Some(heading);
        if self.turned > std::f32::consts::TAU {
            // Orbiting: back out on opposite lock once.
            self.turned = 0.0;
            self.reverse_time = 1.0;
        }

        if car.is_flipping() {
            return DriveInput::default();
        }
        self.since_progress += dt;
        if !car.is_upright() {
            self.flipped_time += dt;
        } else {
            self.flipped_time = 0.0;
            if car.forward_speed() > 300.0 {
                self.flips = 0;
            }
        }
        // Over for 3 s: Flip Car, like the game's AI (it needs the car to be
        // touching something).
        if self.flipped_time > FLIP_AFTER && self.flips < MAX_FLIPS && car.flip() {
            self.flipped_time = 0.0;
            self.flips += 1;
            self.reverse_time = 0.0;
            return DriveInput::default();
        }
        // A flip can't start or keeps failing, or the car is lost (e.g.
        // flipped back onto its wheels below a raised road and can't get
        // back up): put it back on the racing line, as the game's reposition
        // does.
        let lost = self.since_progress > LOST_AFTER;
        if lost || self.flipped_time > 2.0 * FLIP_AFTER || (self.flips >= MAX_FLIPS && self.flipped_time > FLIP_AFTER) {
            self.flipped_time = 0.0;
            self.flips = 0;
            self.since_progress = 0.0;
            self.reverse_time = 0.0;
            // A lost car goes on to the next node (the game's reposition
            // also places cars past their current node), so it isn't put
            // back at the foot of whatever stopped it.
            if lost {
                self.node = self.next_of(nodes, self.node);
            }
            let next = self.next_of(nodes, self.node);
            let (a, b) = (self.line_point(nodes, self.node), self.line_point(nodes, next));
            let dir = (b - a).normalize_or(Vec3::Z);
            let rot = Quat::from_rotation_y(dir.x.atan2(dir.z));
            car.place_on_ground(world, a - Vec3::Y * 100.0, rot);
            return DriveInput::default();
        }
        if !car.is_upright() {
            return DriveInput::default();
        }

        // Steering: pure pursuit toward a racing-line point ahead.
        let speed = car.forward_speed();
        let lookahead = 350.0 + speed.abs() * 0.45;
        let target = self.point_ahead(nodes, pos, lookahead);
        let local = car.rot.inverse() * (target - pos);
        let angle = local.x.atan2(local.z); // +x is right in game space
        let mut steer = (angle / 0.45).clamp(-1.0, 1.0);

        // Speed: slow down for the sharpest bend within braking distance.
        let bend = self.bend_ahead(nodes, pos, 500.0 + speed.max(0.0) * 0.9);
        let top = car.setup.top_speed * self.pace;
        let target_speed = top * if bend > 1.0 { 0.45 } else if bend > 0.6 { 0.6 } else if bend > 0.3 { 0.8 } else { 1.0 };
        let mut throttle: f32 = if speed < target_speed {
            1.0
        } else if speed > target_speed * 1.15 {
            -0.6
        } else {
            0.2
        };

        // Stuck against something: back out with opposite lock.
        if self.reverse_time > 0.0 {
            self.reverse_time -= dt;
            return DriveInput { throttle: -1.0, steer: -steer };
        }
        if throttle > 0.5 && speed.abs() < 60.0 {
            self.stuck_time += dt;
            if self.stuck_time > 1.5 {
                self.stuck_time = 0.0;
                self.reverse_time = 1.0;
            }
        } else {
            self.stuck_time = 0.0;
        }
        // Target behind: turn hard. If it's inside the turning circle the
        // car circles, gains no ground, and the stall reset recovers it.
        if local.z < 0.0 {
            steer = if angle > 0.0 { 1.0 } else { -1.0 };
            throttle = throttle.min(0.6);
        }
        DriveInput { throttle, steer }
    }

    /// Moves `node` forward while the car is past the end of its segment;
    /// re-finds the nearest node if the car is far away (e.g. after a reset).
    fn advance(&mut self, nodes: &AiNodes, pos: Vec3) {
        // Lost (reset, knocked off the line): re-find. Measured to the whole
        // segment, since straights can be thousands of units long.
        let next = self.peek_next(nodes, self.node);
        if segment_distance(self.line_point(nodes, self.node), self.line_point(nodes, next), pos) > 2500.0 {
            self.node = nearest_node(nodes, pos);
        }
        for _ in 0..8 {
            let next = self.peek_next(nodes, self.node);
            let (a, b) = (self.line_point(nodes, self.node), self.line_point(nodes, next));
            let ab = b - a;
            let t = (pos - a).dot(ab) / ab.length_squared().max(1.0);
            if t < 1.0 {
                return;
            }
            self.node = self.next_of(nodes, self.node);
        }
    }

    /// The next node along this driver's route. At forks it takes the branch
    /// whose first node is nearest in height (some branches continue from
    /// the top of a jump, which a car on the ground can't reach), else the
    /// first listed. What the per-node priority byte means for route choice
    /// is not known.
    fn peek_next(&self, nodes: &AiNodes, i: usize) -> usize {
        let centre = |j: usize| (Vec3::from(nodes.nodes[j].left) + Vec3::from(nodes.nodes[j].right)) * 0.5;
        let here = centre(i).y;
        let mut best: Option<(usize, f32)> = None;
        for &j in nodes.nodes[i].next.iter().filter(|&&j| j >= 0 && (j as usize) < nodes.nodes.len()) {
            let rise = (centre(j as usize).y - here).abs();
            if best.is_none_or(|(_, r)| rise < r - 30.0) {
                best = Some((j as usize, rise));
            }
        }
        best.map_or(i, |(j, _)| j)
    }

    fn point_ahead(&self, nodes: &AiNodes, pos: Vec3, mut distance: f32) -> Vec3 {
        let mut i = self.node;
        let mut from = pos;
        for _ in 0..32 {
            let next = self.peek_next(nodes, i);
            let to = self.line_point(nodes, next);
            let seg = to.distance(from);
            if seg >= distance {
                return from + (to - from) * (distance / seg.max(1e-3));
            }
            distance -= seg;
            from = to;
            i = next;
        }
        from
    }

    /// Largest heading change (radians) between consecutive racing-line
    /// segments within `distance` ahead.
    fn bend_ahead(&self, nodes: &AiNodes, pos: Vec3, distance: f32) -> f32 {
        let mut i = self.node;
        let mut travelled = 0.0;
        let mut prev_dir: Option<Vec3> = None;
        let mut from = pos;
        let mut worst = 0.0f32;
        for _ in 0..32 {
            let next = self.peek_next(nodes, i);
            let to = self.line_point(nodes, next);
            let d = to - from;
            let flat = Vec3::new(d.x, 0.0, d.z).normalize_or_zero();
            if let Some(p) = prev_dir {
                worst = worst.max(p.dot(flat).clamp(-1.0, 1.0).acos());
            }
            prev_dir = Some(flat);
            travelled += d.length();
            if travelled > distance {
                break;
            }
            from = to;
            i = next;
        }
        worst
    }
}

/// Nearest node centre, with height differences weighted heavily so a car
/// never latches onto a deck above or below it.
fn nearest_node(nodes: &AiNodes, pos: Vec3) -> usize {
    let cost = |i: usize| {
        let c = (Vec3::from(nodes.nodes[i].left) + Vec3::from(nodes.nodes[i].right)) * 0.5;
        let d = c - pos;
        d.x * d.x + d.z * d.z + 16.0 * d.y * d.y
    };
    (0..nodes.nodes.len()).min_by(|&a, &b| cost(a).total_cmp(&cost(b))).unwrap_or(0)
}

fn segment_distance(a: Vec3, b: Vec3, p: Vec3) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1.0)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}
