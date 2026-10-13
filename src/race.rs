//! Lap counting and race standings from a level's track zones (`.taz`) and
//! position nodes (`.pan`).
//!
//! The rules here are inferred from the data, not ported: zones carry ids in
//! racing order (parallel boxes on forks share an id), a lap completes when a
//! car moves from the last zone into zone 0 having passed the halfway zone
//! since its previous lap, and standings use the distance each car has
//! covered along the position-node graph, unwrapped across the finish line.

use glam::Vec3;

use crate::formats::pan::PosNodes;
use crate::formats::taz::Zone;

pub struct Track {
    zones: Vec<Zone>,
    /// Number of distinct zone ids (ids run 0..zone_count).
    zone_count: i32,
    nodes: PosNodes,
}

impl Track {
    /// `None` for levels that can't be raced (battle arenas, stunt arena).
    pub fn new(zones: Vec<Zone>, nodes: PosNodes) -> Option<Self> {
        let zone_count = zones.iter().map(|z| z.id).max()? + 1;
        if zone_count < 3 || nodes.nodes.len() < 2 || nodes.total <= 0.0 {
            return None;
        }
        Some(Track { zones, zone_count, nodes })
    }

    pub fn lap_length(&self) -> f32 {
        self.nodes.total
    }

    /// Ids of every zone containing `p` (zones overlap where the track
    /// doubles back).
    pub fn zones_at(&self, p: Vec3) -> Vec<i32> {
        self.zones.iter().filter(|z| z.contains(p.to_array())).map(|z| z.id).collect()
    }

    /// Zone id containing `p` (game space), if any.
    pub fn zone_at(&self, p: Vec3) -> Option<i32> {
        self.zones.iter().find(|z| z.contains(p.to_array())).map(|z| z.id)
    }

    fn nearest_node(&self, p: Vec3) -> usize {
        let nodes = &self.nodes.nodes;
        (0..nodes.len())
            .min_by(|&a, &b| Vec3::from(nodes[a].pos).distance_squared(p).total_cmp(&Vec3::from(nodes[b].pos).distance_squared(p)))
            .unwrap()
    }

    /// Distance to the finish at `p`, interpolated along the racing-line
    /// segment nearest to `p` among those within two links of `near`.
    /// Returns the distance and the node to search from next time. Searching
    /// locally keeps cars on their own leg where the track doubles back.
    pub fn locate(&self, p: Vec3, near: Option<usize>) -> (f32, usize, Vec3) {
        let nodes = &self.nodes.nodes;
        let links = |i: usize| -> Vec<usize> {
            let n = &nodes[i];
            n.next.iter().chain(&n.prev).filter(|&&j| j >= 0 && (j as usize) < nodes.len()).map(|&j| j as usize).collect()
        };
        let start = near.filter(|&i| i < nodes.len()).unwrap_or_else(|| self.nearest_node(p));
        let mut candidates = vec![start];
        for depth in 0..2 {
            let frontier: Vec<usize> = candidates.clone();
            for i in frontier {
                candidates.extend(links(i));
            }
            let _ = depth;
        }
        candidates.sort_unstable();
        candidates.dedup();

        let total = self.nodes.total;
        let mut best: Option<(f32, f32, usize, Vec3)> = None; // (distance², dist-to-finish, node, direction)
        for &a in &candidates {
            for &b in nodes[a].next.iter().filter(|&&j| j >= 0 && (j as usize) < nodes.len()) {
                let b = b as usize;
                let (pa, pb) = (Vec3::from(nodes[a].pos), Vec3::from(nodes[b].pos));
                let ab = pb - pa;
                let t = ((p - pa).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                let d2 = (pa + ab * t).distance_squared(p);
                if best.is_some_and(|(bd, ..)| bd <= d2) {
                    continue;
                }
                let (da, mut db) = (nodes[a].dist, nodes[b].dist);
                // The segment across the finish line wraps from ~0 to ~total.
                if (da - db).abs() > total * 0.5 {
                    db += if db < da { total } else { -total };
                }
                let dist = (da + (db - da) * t).rem_euclid(total);
                best = Some((d2, dist, if t > 0.5 { b } else { a }, ab.normalize_or_zero()));
            }
        }
        match best {
            // Far from every local segment (e.g. after a reset): search globally.
            Some((d2, ..)) if near.is_some() && d2 > 1500.0 * 1500.0 => self.locate(p, None),
            Some((_, dist, node, dir)) => (dist, node, dir),
            None => (nodes[start].dist, start, Vec3::ZERO),
        }
    }

    /// Distance to the finish line at `p` (global search).
    pub fn dist_to_finish(&self, p: Vec3) -> f32 {
        self.locate(p, None).0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LapEvent {
    /// Completed lap number `n` (1-based) in `time` seconds.
    Lap { n: u32, time: f32 },
}

#[derive(Clone, Debug, Default)]
pub struct LapTracker {
    /// Last zone id passed in order.
    zone: i32,
    /// Passed the halfway zone since the last lap.
    halfway: bool,
    pub laps: u32,
    pub lap_time: f32,
    pub best_lap: Option<f32>,
    /// Progress has been falling for a while.
    pub wrong_way: bool,
    /// Racing-line node the car was last located at.
    node: Option<usize>,
    /// Racing-line distance covered since the start, unwrapped across the
    /// finish line (negative while still behind it on the grid).
    distance: f32,
    /// Distance into the lap at the last update.
    into_lap: f32,
    last_pos: Vec3,
    backwards_time: f32,
}

impl LapTracker {
    pub fn new(track: &Track, start: Vec3) -> Self {
        let (dist, node, _) = track.locate(start, None);
        let total = track.lap_length();
        let into_lap = total - dist;
        // A grid behind the line starts just short of zero.
        let distance = if into_lap > total * 0.5 { into_lap - total } else { into_lap };
        LapTracker {
            zone: track.zone_at(start).unwrap_or(track.zone_count - 1),
            node: Some(node),
            distance,
            into_lap,
            last_pos: start,
            ..Default::default()
        }
    }

    pub fn update(&mut self, track: &Track, pos: Vec3, dt: f32) -> Option<LapEvent> {
        self.lap_time += dt;
        let (dist, node, dir) = track.locate(pos, self.node);
        self.node = Some(node);
        let total = track.lap_length();
        let into_lap = total - dist;
        let mut delta = into_lap - self.into_lap;
        if delta < -total * 0.5 {
            delta += total; // crossed the finish line forwards
        } else if delta > total * 0.5 {
            delta -= total; // crossed it backwards
        }
        self.into_lap = into_lap;
        self.distance += delta;
        let event = self.update_zones(track, pos);

        // Wrong way: moving against the racing line for most of a second.
        let moved = pos - std::mem::replace(&mut self.last_pos, pos);
        let along = moved.dot(dir);
        if along < -0.3 * moved.length() && moved.length() > dt * 100.0 {
            self.backwards_time += dt;
        } else if along > 0.0 {
            self.backwards_time = 0.0;
        }
        self.wrong_way = self.backwards_time > 0.75;
        event
    }

    fn update_zones(&mut self, track: &Track, pos: Vec3) -> Option<LapEvent> {
        // Zones overlap, so consider every zone containing the car and prefer
        // the one that continues the lap in order.
        let inside = track.zones_at(pos);
        let last = track.zone_count - 1;
        // One zone may be skipped between samples (gaps between boxes).
        if let Some(&z) = inside.iter().filter(|&&z| z == self.zone + 1 || z == self.zone + 2).min() {
            self.zone = z;
            if z >= track.zone_count / 2 {
                self.halfway = true;
            }
            None
        } else if (self.zone == last || self.zone == last - 1) && inside.contains(&0) {
            self.zone = 0;
            if !self.halfway {
                return None; // crossed the line at the start of the race
            }
            self.halfway = false;
            self.laps += 1;
            let time = std::mem::take(&mut self.lap_time);
            self.best_lap = Some(self.best_lap.map_or(time, |b| b.min(time)));
            Some(LapEvent::Lap { n: self.laps, time })
        } else {
            None
        }
    }

    /// Racing-line distance covered since the start, for ranking cars.
    pub fn progress(&self) -> f32 {
        self.distance
    }
}
