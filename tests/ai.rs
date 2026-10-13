//! AI drivers must get round every race track on their own.

mod common;

use glam::{Quat, Vec3};
use revolt_web::ai::AiDriver;
use revolt_web::car::{Car, CarSetup};
use revolt_web::collision::level_world;
use revolt_web::race::{LapEvent, LapTracker, Track};

#[test]
fn ai_completes_a_lap_on_every_race_track() {
    let Some(game) = common::open_game() else { return };
    let car_assets = pollster::block_on(game.load_car("RC")).unwrap();
    let (lo, hi) = car_assets.body_bounds();
    let setup = CarSetup::new(&car_assets.params, car_assets.hull.as_ref(), lo, hi);
    let mut failures = Vec::new();
    for entry in pollster::block_on(game.list_levels()).unwrap() {
        let level = pollster::block_on(game.load_level(&entry.id)).unwrap();
        let Some(track) = Track::new(level.zones.clone(), level.pos_nodes.clone()) else { continue };
        let world = level_world(&level);
        let start = Vec3::from(level.info.start_pos);
        let rot = Quat::from_rotation_y(level.info.start_rot * std::f32::consts::TAU);
        let mut car = Car::new(setup.clone(), start, rot);
        car.place_on_ground(&world, start, rot);
        let mut driver = AiDriver::new(&level.ai_nodes, car.pos, 7);
        let mut tracker = LapTracker::new(&track, car.pos);
        let dt = 1.0 / 60.0;
        let mut lap_time = None;
        let mut resets = 0;
        for frame in 0..(240.0 / dt) as usize {
            let was_upright = car.is_upright();
            let input = driver.drive(&level.ai_nodes, &world, &mut car, dt);
            if !was_upright && car.is_upright() {
                resets += 1;
            }
            car.update(&world, input, dt);
            if let Some(LapEvent::Lap { time, .. }) = tracker.update(&track, car.pos, dt) {
                lap_time = Some(time);
                let _ = frame;
                break;
            }
        }
        println!("{:14} lap {:?} resets {resets} progress {:.0}/{:.0}", entry.id, lap_time.map(|t| format!("{t:.1}s")), tracker.progress(), track.lap_length());
        if lap_time.is_none() {
            failures.push(entry.id.clone());
        }
    }
    assert!(failures.is_empty(), "AI did not finish a lap on {failures:?}");
}
