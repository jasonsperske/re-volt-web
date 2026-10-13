//! Drives cars on real tracks from the disc and checks the physics behaves:
//! settles on the ground, accelerates forward, steers the right way, and
//! never falls through the world.

mod common;

use glam::{Quat, Vec3};
use revolt_web::car::{Car, CarSetup, DriveInput};
use revolt_web::collision::{level_world, CollisionWorld, NCP_OBJECTS_IGNORE};

fn spawn(game: &revolt_web::assets::GameData<common::FileSource>, level: &str, car: &str) -> (Car, CollisionWorld) {
    let level = pollster::block_on(game.load_level(level)).unwrap();
    let car = pollster::block_on(game.load_car(car)).unwrap();
    let (lo, hi) = car.body_bounds();
    let setup = CarSetup::new(&car.params, car.hull.as_ref(), lo, hi);
    let rot = Quat::from_rotation_y(level.info.start_rot * std::f32::consts::TAU);
    let world = level_world(&level);
    let mut car = Car::new(setup, Vec3::ZERO, Quat::IDENTITY);
    car.place_on_ground(&world, Vec3::from(level.info.start_pos), rot);
    (car, world)
}

fn run(car: &mut Car, world: &CollisionWorld, input: DriveInput, seconds: f32) {
    for _ in 0..(seconds * 60.0) as usize {
        car.update(world, input, 1.0 / 60.0);
    }
}

#[test]
fn rc_bandit_settles_drives_and_steers_on_toys_in_the_hood() {
    let Some(game) = common::open_game() else { return };
    let (mut car, world) = spawn(&game, "NHOOD1", "RC");

    run(&mut car, &world, DriveInput::default(), 2.0);
    let ground = world.raycast(car.pos, Vec3::Y, 200.0, NCP_OBJECTS_IGNORE).expect("ground under the car");
    println!("settled: pos {:?} speed {:.1} ground {:.1} below", car.pos, car.vel.length(), ground.t);
    assert!(car.is_upright());
    assert!(car.vel.length() < 30.0, "still moving at {}", car.vel.length());
    assert!(car.wheels.iter().all(|w| w.contact), "all wheels should touch the road");
    // At rest the wheels carry exactly the car's weight each step.
    let load: f32 = car.wheels.iter().map(|w| w.load).sum();
    let weight = car.setup.mass * 2200.0 * revolt_web::car::DT;
    println!("wheel loads {load:.2} vs weight impulse {weight:.2}");
    assert!((load - weight).abs() < weight * 0.05, "loads {load} vs weight {weight}");

    let (start, heading) = (car.pos, car.forward());
    run(&mut car, &world, DriveInput { throttle: 1.0, steer: 0.0 }, 2.0);
    let travelled = (car.pos - start).dot(heading);
    let top = car.forward_speed();
    println!("full throttle 2s: {travelled:.0} units forward, {:.1} mph", car.speed_mph());
    assert!(travelled > 1000.0, "only moved {travelled}");
    assert!(car.speed_mph() > 15.0);
    assert!(car.is_upright());

    // Braking from speed: the original brakes by driving the wheels backwards.
    run(&mut car, &world, DriveInput { throttle: -1.0, steer: 0.0 }, 0.6);
    println!("braking 0.6s: {:.0} -> {:.0} units/s", top, car.forward_speed());
    assert!(car.forward_speed() < top * 0.5, "barely slowed: {}", car.forward_speed());
    run(&mut car, &world, DriveInput { throttle: -1.0, steer: 0.0 }, 1.5);
    println!("then reversing: {:.0} units/s", car.forward_speed());
    assert!(car.forward_speed() < -100.0, "should be reversing");
    assert!(car.is_upright());

    // Steering, from a fresh start.
    let (mut car, world) = spawn(&game, "NHOOD1", "RC");
    run(&mut car, &world, DriveInput::default(), 1.0);
    run(&mut car, &world, DriveInput { throttle: 1.0, steer: 0.0 }, 0.5);
    let before = car.forward();
    run(&mut car, &world, DriveInput { throttle: 0.5, steer: 1.0 }, 1.0);
    // Game space is Y-down, so a right turn is a positive rotation about +Y.
    let turn = before.cross(car.forward()).y;
    println!("steer right 1s: turn {turn:.2}");
    assert!(turn > 0.2, "car did not turn right ({turn})");
    assert!(car.is_upright());
}

#[test]
fn every_track_keeps_cars_on_their_wheels() {
    let Some(game) = common::open_game() else { return };
    let levels = pollster::block_on(game.list_levels()).unwrap();
    let mut crashes = Vec::new();
    for entry in &levels {
        for car_id in ["RC", "TC4", "MITE"] {
            let (mut car, world) = spawn(&game, &entry.id, car_id);
            let start = car.pos;
            // Settling and a short straight run must never tip a car over.
            run(&mut car, &world, DriveInput::default(), 1.5);
            run(&mut car, &world, DriveInput { throttle: 1.0, steer: 0.0 }, 0.75);
            assert!(car.is_upright(), "{} {car_id}: rolled over on the start straight", entry.id);
            // Then drive blind into whatever is ahead: crashes may flip the
            // car (crash response is still approximate), but it must never
            // leave the world.
            run(&mut car, &world, DriveInput { throttle: 1.0, steer: 0.3 }, 2.25);
            let below = world.raycast(car.pos - Vec3::Y * 50.0, Vec3::Y, 5000.0, NCP_OBJECTS_IGNORE);
            println!(
                "{:14} {:5} moved {:6.0} upright {} ground below {}",
                entry.id, car_id, (car.pos - start).length(), car.is_upright(), below.is_some()
            );
            assert!(car.pos.y - start.y < 1500.0, "{} {car_id}: fell to y={}", entry.id, car.pos.y);
            assert!(below.is_some(), "{} {car_id}: nothing under the car", entry.id);
            if !car.is_upright() {
                crashes.push(format!("{} {car_id}", entry.id));
            }
        }
    }
    println!("rolled over after crashing: {crashes:?}");
}

#[test]
fn oil_lengthens_braking() {
    let Some(game) = common::open_game() else { return };
    let stop = |oil: bool| {
        let (mut car, world) = spawn(&game, "NHOOD1", "RC");
        run(&mut car, &world, DriveInput::default(), 1.0);
        run(&mut car, &world, DriveInput { throttle: 1.0, steer: 0.0 }, 1.5);
        if oil {
            let p = car.pos;
            car.set_oil_zones(&[revolt_web::car::OilZone { x: p.x, z: p.z, radius: 3000.0, y_min: p.y - 500.0, y_max: p.y + 500.0 }]);
        }
        let start = car.pos;
        run(&mut car, &world, DriveInput { throttle: -1.0, steer: 0.0 }, 0.5);
        (car.pos - start).length()
    };
    let (dry, oily) = (stop(false), stop(true));
    println!("braking distance over 0.5 s: dry {dry:.0}, oil {oily:.0}");
    assert!(oily > dry * 1.1, "oil should reduce braking grip (dry {dry}, oil {oily})");
}

#[test]
fn flip_car_rights_a_car_on_its_roof_without_moving_it_far() {
    let Some(game) = common::open_game() else { return };
    let (mut car, world) = spawn(&game, "NHOOD1", "RC");
    run(&mut car, &world, DriveInput::default(), 1.0);
    let heading = car.forward();

    // Upside down on the road, left to settle on its roof.
    let roof = Quat::from_axis_angle(heading, std::f32::consts::PI) * car.rot;
    car.place(car.pos - Vec3::Y * 30.0, roof);
    run(&mut car, &world, DriveInput::default(), 1.5);
    assert!(!car.is_upright(), "should be lying on its roof");
    let lying = car.pos;

    assert!(car.flip(), "Flip Car should start on the roof");
    assert!(!car.flip(), "a second press while flipping does nothing");
    let mut t = 0.0;
    while car.is_flipping() && t < 3.0 {
        run(&mut car, &world, DriveInput::default(), 1.0 / 60.0);
        t += 1.0 / 60.0;
    }
    println!("flipped in {t:.2} s");
    assert!(!car.is_flipping() && t < 1.0, "flip took {t} s");
    run(&mut car, &world, DriveInput::default(), 1.5);
    let moved = Vec3::new(car.pos.x - lying.x, 0.0, car.pos.z - lying.z).length();
    println!("after landing: upright {}, wheels down {}, moved {moved:.1}", car.is_upright(), car.wheels.iter().all(|w| w.contact));
    assert!(car.is_upright());
    assert!(car.wheels.iter().all(|w| w.contact));
    assert!(car.forward().dot(heading) > 0.95, "keeps its heading");
    assert!(moved < 40.0, "should land where it was, moved {moved}");

    // Already on its wheels: nothing to flip.
    assert!(!car.flip());
}

#[test]
fn workbench_tuning_changes_how_the_car_drives() {
    let Some(game) = common::open_game() else { return };
    let level = pollster::block_on(game.load_level("NHOOD1")).unwrap();
    let world = level_world(&level);
    let rot = Quat::from_rotation_y(level.info.start_rot * std::f32::consts::TAU);
    let build = |tuning: &revolt_web::tuning::Tuning| {
        let mut car = pollster::block_on(game.load_car("RC")).unwrap();
        tuning.apply(&mut car.params);
        let (lo, hi) = car.body_bounds();
        let mut sim = Car::new(CarSetup::new(&car.params, car.hull.as_ref(), lo, hi), Vec3::ZERO, Quat::IDENTITY);
        sim.place_on_ground(&world, Vec3::from(level.info.start_pos), rot);
        run(&mut sim, &world, DriveInput::default(), 2.0);
        sim
    };
    let ride = |car: &Car| world.raycast(car.pos, Vec3::Y, 200.0, NCP_OBJECTS_IGNORE).unwrap().t;
    let speed_after = |mut car: Car| {
        run(&mut car, &world, DriveInput { throttle: 1.0, steer: 0.0 }, 3.0);
        car.speed_mph()
    };

    let stock = build(&Default::default());
    let mut soft = revolt_web::tuning::Tuning::default();
    soft.set("stiffness", 0.5);
    soft.set("mass", 1.5);
    let soft = build(&soft);
    println!("ride height: stock {:.1}, soft and heavy {:.1}", ride(&stock), ride(&soft));
    assert!(ride(&soft) < ride(&stock) - 1.0, "softer springs and more weight should sit lower");

    let mut slow = revolt_web::tuning::Tuning::default();
    slow.set("top_speed", 0.5);
    let (fast_mph, slow_mph) = (speed_after(build(&Default::default())), speed_after(build(&slow)));
    println!("speed after 3 s: stock {fast_mph:.1} mph, half top speed {slow_mph:.1} mph");
    assert!(slow_mph < fast_mph * 0.75);
}
