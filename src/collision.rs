//! Static collision world built from a level's `.ncp` plus its instances'
//! `.ncp` files. Works in game space (Y down).

use glam::{Mat3, Vec3, Vec4};

use crate::formats::ncp::CollPoly;

const CELL: f32 = 256.0;

#[derive(Clone, Debug)]
pub struct Poly {
    pub plane: Vec4,
    pub edges: [Vec4; 4],
    pub edge_count: usize,
    pub material: i32,
    pub flags: i32,
    pub min: Vec3,
    pub max: Vec3,
}

impl Poly {
    fn from_ncp(p: &CollPoly) -> Self {
        Poly {
            plane: Vec4::from(p.plane),
            edges: p.edges.map(Vec4::from),
            edge_count: p.edge_count(),
            material: p.material,
            flags: p.flags,
            // The game grows each box by 2 units when loading.
            min: Vec3::new(p.bbox[0], p.bbox[2], p.bbox[4]) - 2.0,
            max: Vec3::new(p.bbox[1], p.bbox[3], p.bbox[5]) + 2.0,
        }
    }

    /// Moves the polygon by `p' = rot * p + pos`.
    fn transformed(&self, rot: Mat3, pos: Vec3) -> Self {
        let plane = |pl: Vec4| {
            let n = rot * pl.truncate();
            n.extend(pl.w - n.dot(pos))
        };
        let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for i in 0..8 {
            let corner = Vec3::new(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            );
            let c = rot * corner + pos;
            min = min.min(c);
            max = max.max(c);
        }
        Poly { plane: plane(self.plane), edges: self.edges.map(plane), min, max, ..*self }
    }

    pub fn normal(&self) -> Vec3 {
        self.plane.truncate()
    }

    pub fn distance(&self, p: Vec3) -> f32 {
        self.plane.truncate().dot(p) + self.plane.w
    }

    /// Whether a point on (or near) the plane lies within the polygon's edges.
    pub fn contains(&self, p: Vec3) -> bool {
        self.edges[..self.edge_count].iter().all(|e| e.truncate().dot(p) + e.w <= 0.01)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    pub point: Vec3,
    pub normal: Vec3,
    pub material: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct Contact {
    /// Surface normal, pointing out of the solid.
    pub normal: Vec3,
    /// Penetration depth (positive when overlapping).
    pub depth: f32,
    /// Closest point on the polygon's plane.
    pub point: Vec3,
    pub material: i32,
    pub flags: i32,
}

/// NCP type bits (see docs/physics.md §5).
pub const NCP_CAMERA_IGNORES: i32 = 0x4;
/// Camera-only surfaces: cars and objects pass through them.
pub const NCP_OBJECTS_IGNORE: i32 = 0x8;
pub const NCP_NO_SKID: i32 = 0x20;

pub struct CollisionWorld {
    pub polys: Vec<Poly>,
    origin: (f32, f32),
    dims: (usize, usize),
    cells: Vec<Vec<u32>>,
}

impl CollisionWorld {
    /// `level` is the track `.ncp`; `instances` are prop collision meshes
    /// with their placement.
    pub fn new(level: &[CollPoly], instances: &[(&[CollPoly], Mat3, Vec3)]) -> Self {
        let mut polys: Vec<Poly> = level.iter().map(Poly::from_ncp).collect();
        for (src, rot, pos) in instances {
            polys.extend(src.iter().map(|p| Poly::from_ncp(p).transformed(*rot, *pos)));
        }
        let (mut min, mut max) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
        for p in &polys {
            min = (min.0.min(p.min.x), min.1.min(p.min.z));
            max = (max.0.max(p.max.x), max.1.max(p.max.z));
        }
        if polys.is_empty() {
            (min, max) = ((0.0, 0.0), (1.0, 1.0));
        }
        let dims = (((max.0 - min.0) / CELL) as usize + 1, ((max.1 - min.1) / CELL) as usize + 1);
        let mut world = CollisionWorld { polys, origin: min, dims, cells: vec![Vec::new(); dims.0 * dims.1] };
        for i in 0..world.polys.len() {
            let (lo, hi) = (world.polys[i].min, world.polys[i].max);
            let cells: Vec<usize> = world.cell_range(lo, hi).collect();
            for cell in cells {
                world.cells[cell].push(i as u32);
            }
        }
        world
    }

    fn cell_range(&self, lo: Vec3, hi: Vec3) -> impl Iterator<Item = usize> + '_ {
        let clamp_x = |v: f32| (((v - self.origin.0) / CELL).floor().max(0.0) as usize).min(self.dims.0 - 1);
        let clamp_z = |v: f32| (((v - self.origin.1) / CELL).floor().max(0.0) as usize).min(self.dims.1 - 1);
        let (x0, x1, z0, z1) = (clamp_x(lo.x), clamp_x(hi.x), clamp_z(lo.z), clamp_z(hi.z));
        (z0..=z1).flat_map(move |z| (x0..=x1).map(move |x| z * self.dims.0 + x))
    }

    /// Candidate polygons whose boxes overlap the query box (deduplicated).
    fn candidates(&self, lo: Vec3, hi: Vec3, out: &mut Vec<u32>) {
        out.clear();
        for cell in self.cell_range(lo, hi) {
            for &i in &self.cells[cell] {
                let p = &self.polys[i as usize];
                if p.max.cmpge(lo).all() && p.min.cmple(hi).all() {
                    out.push(i);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// Nearest front-facing hit along `origin + dir * t`, `0 <= t <= max_t`,
    /// ignoring polygons with any of the `skip` type bits.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_t: f32, skip: i32) -> Option<Hit> {
        let end = origin + dir * max_t;
        let mut cand = Vec::new();
        self.candidates(origin.min(end), origin.max(end), &mut cand);
        let mut best: Option<Hit> = None;
        for &i in &cand {
            let p = &self.polys[i as usize];
            if p.flags & skip != 0 {
                continue;
            }
            let n = p.normal();
            let denom = n.dot(dir);
            if denom >= -1e-6 {
                continue; // parallel or hitting the back
            }
            let t = -p.distance(origin) / denom;
            if t < 0.0 || t > max_t || best.is_some_and(|b| t >= b.t) {
                continue;
            }
            let point = origin + dir * t;
            if p.contains(point) {
                best = Some(Hit { t, point, normal: n, material: p.material });
            }
        }
        best
    }

    /// Faces the sphere penetrates from the front. Contacts deeper than the
    /// radius are kept down to `-radius` so a fast body can't tunnel through.
    pub fn sphere_contacts(&self, center: Vec3, radius: f32, skip: i32, out: &mut Vec<Contact>) {
        out.clear();
        let mut cand = Vec::new();
        self.candidates(center - radius, center + radius, &mut cand);
        for &i in &cand {
            let p = &self.polys[i as usize];
            if p.flags & skip != 0 {
                continue;
            }
            let dist = p.distance(center);
            if dist >= radius || dist < -radius {
                continue;
            }
            let point = center - p.normal() * dist;
            if p.contains(point) {
                out.push(Contact { normal: p.normal(), depth: radius - dist, point, material: p.material, flags: p.flags });
            }
        }
    }
}

/// Collision for a loaded level: the track plus every placed prop that ships
/// a collision mesh.
pub fn level_world(level: &crate::assets::LevelAssets) -> CollisionWorld {
    let instances: Vec<(&[CollPoly], Mat3, Vec3)> = level
        .instances
        .iter()
        .filter_map(|(inst, _)| {
            let polys = level.instance_collision.get(&inst.name)?;
            Some((polys.as_slice(), crate::scene::instance_rotation(&inst.rot), Vec3::from(inst.pos)))
        })
        .collect();
    CollisionWorld::new(&level.collision, &instances)
}
