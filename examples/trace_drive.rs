//! Traces a car's state over time on a level (debugging aid).
//! Usage: trace_drive LEVEL CAR
#[path = "../tests/common/mod.rs"]
mod common;
use glam::{Quat, Vec3};
use revolt_web::car::{Car, CarSetup, DriveInput};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let game = common::open_game().expect("disc");
    let level = pollster::block_on(game.load_level(&a[1])).unwrap();
    let assets = pollster::block_on(game.load_car(&a[2])).unwrap();
    let (lo, hi) = assets.body_bounds();
    let world = revolt_web::collision::level_world(&level);
    let rot = Quat::from_rotation_y(level.info.start_rot * std::f32::consts::TAU);
    let mut car = Car::new(CarSetup::new(&assets.params, assets.hull.as_ref(), lo, hi), Vec3::ZERO, Quat::IDENTITY);
    car.place_on_ground(&world, Vec3::from(level.info.start_pos), rot);
    for frame in 0..300 {
        let steer = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let input = if frame < 90 { DriveInput::default() } else { DriveInput { throttle: 1.0, steer } };
        car.update(&world, input, 1.0 / 60.0);
        if frame % 15 == 0 {
            let comp: Vec<String> = car.wheels.iter().map(|w| format!("{}{:4.1}", if w.contact { "*" } else { " " }, w.susp_pos)).collect();
            println!(
                "t {:4.2} thr {} fwd {:7.1} vel {:6.1?} up·down {:5.2} angvel {:5.2?} rot {:5.3?} wheels [{}]",
                frame as f32 / 60.0, input.throttle, car.forward_speed(), car.vel.to_array(),
                (car.rot * Vec3::Y).y, car.ang_vel.to_array(), car.rot.to_array(), comp.join(" ")
            );
        }
    }
}
