//! Per-step physics trace (debugging aid). Usage: trace_steps LEVEL CAR [steps] [throttle]
#[path = "../tests/common/mod.rs"]
mod common;
use glam::{Quat, Vec3};
use revolt_web::car::{Car, CarSetup, DriveInput};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let steps: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(40);
    let throttle: f32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let brake_after: usize = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    let game = common::open_game().expect("disc");
    let level = pollster::block_on(game.load_level(&a[1])).unwrap();
    let assets = pollster::block_on(game.load_car(&a[2])).unwrap();
    let (lo, hi) = assets.body_bounds();
    let world = revolt_web::collision::level_world(&level);
    let rot = Quat::from_rotation_y(level.info.start_rot * std::f32::consts::TAU);
    let mut car = Car::new(CarSetup::new(&assets.params, assets.hull.as_ref(), lo, hi), Vec3::ZERO, Quat::IDENTITY);
    car.place_on_ground(&world, Vec3::from(level.info.start_pos), rot);
    for i in 0..steps {
        let throttle = if i >= brake_after { -1.0 } else { throttle };
        car.step_once(&world, DriveInput { throttle, steer: 0.0 });
        let w: Vec<String> = car.wheels.iter().map(|w| format!("{}{:5.1}/{:6.1}/{:5.1}", if w.contact { "*" } else { " " }, w.susp_pos, w.susp_vel, w.load)).collect();
        let spin: Vec<String> = car.wheels.iter().map(|w| format!("{:6.1}", w.ang_vel)).collect();
        println!("{i:3} thr {:5.2} vfwd {:7.1} | {} | spin {}", car.throttle, car.forward_speed(), w.join(" "), spin.join(" "));
    }
}
