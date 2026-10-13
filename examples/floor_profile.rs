//! Samples collision floor height along a line (debugging aid).
//! Usage: floor_profile LEVEL x0 z0 x1 z1 [y_from]
#[path = "../tests/common/mod.rs"]
mod common;
use glam::Vec3;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let f = |i: usize| a[i].parse::<f32>().unwrap();
    let game = common::open_game().expect("disc");
    let level = pollster::block_on(game.load_level(&a[1])).unwrap();
    let world = revolt_web::collision::level_world(&level);
    let y0 = a.get(6).and_then(|s| s.parse().ok()).unwrap_or(0.0f32);
    for k in 0..=40 {
        let t = k as f32 / 40.0;
        let (x, z) = (f(2) + (f(4) - f(2)) * t, f(3) + (f(5) - f(3)) * t);
        let hit = world.raycast(Vec3::new(x, y0, z), Vec3::Y, 5000.0, revolt_web::collision::NCP_OBJECTS_IGNORE);
        println!("x {x:7.0} z {z:7.0} floor y {}", hit.map_or("none".into(), |h| format!("{:7.1} n {:?} mat {}", h.point.y, h.normal.to_array().map(|v| (v * 100.0).round() / 100.0), h.material)));
    }
}
