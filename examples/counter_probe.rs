//! Prints the heights of upward-facing surfaces in a level near a point
//! (debugging aid). Usage: counter_probe LEVEL x z radius [y: list polys at that height]
#[path = "../tests/common/mod.rs"]
mod common;
use glam::Vec3;
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let f = |i: usize| a[i].parse::<f32>().unwrap();
    let (cx, cz, r) = (f(2), f(3), f(4));
    let game = common::open_game().expect("disc");
    let level = pollster::block_on(game.load_level(&a[1])).unwrap();
    let mut by_y: BTreeMap<i32, (f32, f32, f32, f32, usize)> = BTreeMap::new();
    for m in &level.world.meshes {
        for p in &m.mesh.polys {
            let n = if p.flags & 1 != 0 { 4 } else { 3 };
            let v: Vec<Vec3> = (0..n).map(|i| Vec3::from(m.mesh.verts[p.indices[i] as usize].pos)).collect();
            let normal = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero();
            if normal.y.abs() < 0.95 { continue; }
            let c = v.iter().copied().sum::<Vec3>() / n as f32;
            if (c.x - cx).hypot(c.z - cz) > r { continue; }
            if a.get(5).is_some_and(|y| (c.y - y.parse::<f32>().unwrap()).abs() < 1.0) {
                let ang = (c.z - cz).atan2(c.x - cx).to_degrees();
                println!("  poly centre ({:6.0}, {:6.0}) angle {ang:5.0} dist {:4.0}", c.x, c.z, (c.x - cx).hypot(c.z - cz));
            }
            let e = by_y.entry(c.y.round() as i32).or_insert((f32::MAX, f32::MIN, f32::MAX, f32::MIN, 0));
            for q in &v { e.0 = e.0.min(q.x); e.1 = e.1.max(q.x); e.2 = e.2.min(q.z); e.3 = e.3.max(q.z); }
            e.4 += 1;
        }
    }
    for (y, (x0, x1, z0, z1, n)) in by_y {
        println!("y {y:6}  polys {n:4}  x {x0:7.0}..{x1:7.0}  z {z0:7.0}..{z1:7.0}");
    }
}
