//! Traces an AI driver on a level (debugging aid). Usage: trace_ai LEVEL [CAR] [seconds]
#[path = "../tests/common/mod.rs"]
mod common;
use glam::{Quat, Vec3};
use revolt_web::ai::AiDriver;
use revolt_web::car::{Car, CarSetup};
use revolt_web::race::{LapTracker, Track};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let car_id = a.get(2).cloned().unwrap_or("RC".into());
    let secs: f32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(40.0);
    let game = common::open_game().expect("disc");
    let level = pollster::block_on(game.load_level(&a[1])).unwrap();
    let ca = pollster::block_on(game.load_car(&car_id)).unwrap();
    let (lo, hi) = ca.body_bounds();
    let world = revolt_web::collision::level_world(&level);
    let track = Track::new(level.zones.clone(), level.pos_nodes.clone()).unwrap();
    let start = Vec3::from(level.info.start_pos);
    let rot = Quat::from_rotation_y(level.info.start_rot * std::f32::consts::TAU);
    let mut car = Car::new(CarSetup::new(&ca.params, ca.hull.as_ref(), lo, hi), start, rot);
    car.place_on_ground(&world, start, rot);
    let mut d = AiDriver::new(&level.ai_nodes, car.pos, 7);
    let mut t = LapTracker::new(&track, car.pos);
    let dt = 1.0 / 60.0;
    for f in 0..(secs / dt) as usize {
        let input = d.drive(&level.ai_nodes, &world, &mut car, dt);
        car.update(&world, input, dt);
        t.update(&track, car.pos, dt);
        let tt = f as f32 * dt;
        let detail = a.get(4).and_then(|s| s.parse::<f32>().ok()).is_some_and(|from| tt >= from && tt < from + 0.6);
        if detail {
            let impact = car.take_impact();
            let loads: Vec<String> = car.wheels.iter().map(|w| format!("{}{:5.1}", if w.contact { "*" } else { " " }, w.load)).collect();
            println!("  {tt:6.3} y {:7.1} vy {:7.1} vfwd {:6.0} angvel {:5.2?} up·Y {:5.2} impact {:6.0} loads {}", car.pos.y, car.vel.y, car.forward_speed(), car.ang_vel.to_array(), car.up().y, impact, loads.join(" "));
        }
        if f % 60 == 0 && !detail {
            println!("t {:5.1} node {:3} zones {:?} prog {:7.0} pos {:?} v {:6.0} up {} in thr {:5.2} st {:5.2}", f as f32 * dt, d.node(), track.zones_at(car.pos), t.progress(), car.pos.round(), car.forward_speed(), car.is_upright(), input.throttle, input.steer);
        }
    }
}
