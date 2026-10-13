//! Pickups and each weapon's effect on real cars on Toys in the Hood.

mod common;

use glam::{Quat, Vec3};
use revolt_web::car::{Car, CarSetup, DriveInput};
use revolt_web::collision::{level_world, CollisionWorld};
use revolt_web::weapons::{Weapon, Weapons};

struct Scene {
    world: CollisionWorld,
    cars: Vec<Car>,
    weapons: Weapons,
}

/// Two RC Bandits: car 0 on the start line, car 1 `gap` units ahead of it.
fn scene(gap: f32) -> Scene {
    let game = common::open_game().unwrap();
    let level = pollster::block_on(game.load_level("NHOOD1")).unwrap();
    let assets = pollster::block_on(game.load_car("RC")).unwrap();
    let (lo, hi) = assets.body_bounds();
    let setup = CarSetup::new(&assets.params, assets.hull.as_ref(), lo, hi);
    let world = level_world(&level);
    let start = Vec3::from(level.info.start_pos);
    let rot = Quat::from_rotation_y(level.info.start_rot * std::f32::consts::TAU);
    let mut cars = Vec::new();
    for k in 0..2 {
        let mut car = Car::new(setup.clone(), start, rot);
        car.place_on_ground(&world, start + rot * Vec3::Z * (gap * k as f32), rot);
        cars.push(car);
    }
    let weapons = Weapons::new(&level.objects, 2, 1234);
    let mut s = Scene { world, cars, weapons };
    s.run(1.0, [DriveInput::default(); 2], [false; 2]);
    s
}

impl Scene {
    fn run(&mut self, seconds: f32, inputs: [DriveInput; 2], fire: [bool; 2]) {
        let dt = 1.0 / 60.0;
        for f in 0..(seconds / dt) as usize {
            for (c, input) in self.cars.iter_mut().zip(inputs) {
                c.update(&self.world, input, dt);
            }
            let mut cars: Vec<&mut Car> = self.cars.iter_mut().collect();
            let fire_now = if f == 0 { fire } else { [false; 2] };
            self.weapons.update(&self.world, &mut cars, &[1, 0], &[false, false], &fire_now, dt);
        }
    }

    fn grant(&mut self, i: usize, w: Weapon) {
        let mut cars: Vec<&mut Car> = self.cars.iter_mut().collect();
        self.weapons.grant(i, w, &mut cars);
    }
}

#[test]
fn pickups_appear_after_five_seconds() {
    if common::open_game().is_none() {
        return;
    }
    let mut s = scene(400.0);
    assert_eq!(s.weapons.live_pickups(), 0);
    s.run(4.5, [DriveInput::default(); 2], [false; 2]);
    // 2N+5 = 9 of the level's 27 points.
    println!("live pickups after 5.5 s: {}", s.weapons.live_pickups());
    assert_eq!(s.weapons.live_pickups(), 9);
}

#[test]
fn driving_into_a_pickup_spins_the_roulette_and_awards_a_weapon() {
    if common::open_game().is_none() {
        return;
    }
    let mut s = scene(400.0);
    s.run(5.0, [DriveInput::default(); 2], [false; 2]);
    let target = s.weapons.pickup_positions()[0];
    // Teleport car 0 just short of the pickup and let it settle into it.
    let rot = s.cars[0].rot;
    let world = &s.world;
    s.cars[0].place_on_ground(world, target, rot);
    s.run(0.2, [DriveInput::default(); 2], [false; 2]);
    let a = &s.weapons.armory[0];
    println!("after touching: roulette {:?}, weapon {:?}", a.roulette_display(), a.weapon);
    assert!(a.roulette_display().is_some() || s.weapons.bomb_fuse(0).is_some(), "pickup not collected");
    s.run(4.0, [DriveInput::default(); 2], [false; 2]);
    let a = &s.weapons.armory[0];
    println!("after roulette: {:?} x{}", a.weapon, a.uses);
    assert!(a.weapon.is_some() || s.weapons.bomb_fuse(0).is_some());
}

#[test]
fn shockwave_flips_the_car_ahead() {
    if common::open_game().is_none() {
        return;
    }
    let mut s = scene(600.0);
    s.grant(0, Weapon::Shockwave);
    let before = s.cars[1].pos;
    let (mut lifted, mut spin) = (0.0f32, 0.0f32);
    for k in 0..20 {
        s.run(0.05, [DriveInput::default(); 2], [k == 0, false]);
        lifted = lifted.max(before.y - s.cars[1].pos.y); // y is down
        spin = spin.max(s.cars[1].ang_vel.length());
    }
    println!("shockwave: car ahead rose up to {lifted:.0} units, spun up to {spin:.1} rad/s");
    assert!(lifted > 30.0 && spin > 5.0, "victim unaffected");
}

#[test]
fn firework_explodes_near_the_car_ahead() {
    if common::open_game().is_none() {
        return;
    }
    let mut s = scene(1200.0);
    s.grant(0, Weapon::Firework);
    let before = s.cars[1].pos;
    s.run(2.0, [DriveInput::default(); 2], [true, false]);
    let moved = s.cars[1].pos.distance(before);
    println!("firework: car ahead displaced {moved:.0} units");
    assert!(moved > 30.0, "victim unaffected");
}

#[test]
fn electro_pulse_cuts_power_nearby() {
    if common::open_game().is_none() {
        return;
    }
    let mut s = scene(250.0);
    s.grant(0, Weapon::ElectroPulse);
    s.run(0.5, [DriveInput::default(); 2], [true, false]);
    assert!(s.cars[1].zapped, "car within 384 units should be zapped");
    // A zapped car gets no power.
    let before = s.cars[1].pos;
    s.run(1.0, [DriveInput::default(), DriveInput { throttle: 1.0, steer: 0.0 }], [false; 2]);
    let moved = s.cars[1].pos.distance(before);
    println!("zapped car moved {moved:.0} units on full throttle");
    assert!(moved < 50.0);
}

#[test]
fn turbo_raises_top_speed_and_cures_a_zap() {
    if common::open_game().is_none() {
        return;
    }
    let mut s = scene(2000.0);
    s.grant(0, Weapon::Turbo);
    s.run(0.1, [DriveInput::default(); 2], [true, false]);
    assert!(s.cars[0].top_speed_scale > 1.9, "turbo scale {}", s.cars[0].top_speed_scale);
    s.run(10.5, [DriveInput::default(); 2], [false; 2]);
    assert_eq!(s.cars[0].top_speed_scale, 1.0, "turbo should wear off after 10 s");

    s.grant(1, Weapon::GlobalPulse);
    s.run(0.1, [DriveInput::default(); 2], [false, true]);
    assert!(s.cars[0].zapped);
    s.grant(0, Weapon::Turbo);
    s.run(0.1, [DriveInput::default(); 2], [true, false]);
    assert!(!s.cars[0].zapped && s.cars[0].top_speed_scale == 1.0, "turbo while zapped only cures the zap");
}

#[test]
fn bomb_passes_on_contact_and_explodes_after_its_fuse() {
    if common::open_game().is_none() {
        return;
    }
    let mut s = scene(400.0);
    s.grant(0, Weapon::Bomb);
    assert!(s.weapons.bomb_fuse(0).is_some(), "bomb is live on pickup");
    // Drive into the car ahead to hand it over.
    s.run(3.5, [DriveInput { throttle: 1.0, steer: 0.0 }, DriveInput::default()], [false; 2]);
    println!("bomb carriers: car0 {:?}, car1 {:?}", s.weapons.bomb_fuse(0), s.weapons.bomb_fuse(1));
    assert!(s.weapons.bomb_fuse(1).is_some(), "bomb should pass to the car that was touched");
    let before = s.cars[1].pos;
    s.run(9.0, [DriveInput::default(); 2], [false; 2]);
    assert!(s.weapons.bomb_fuse(1).is_none(), "bomb should have gone off");
    println!("bomb threw the carrier {:.0} units", s.cars[1].pos.distance(before));
}

#[test]
fn oil_slick_is_laid_behind_the_car() {
    if common::open_game().is_none() {
        return;
    }
    let mut s = scene(2000.0);
    s.grant(0, Weapon::OilSlick);
    s.run(1.5, [DriveInput { throttle: 0.5, steer: 0.0 }, DriveInput::default()], [true, false]);
    let oil = s.weapons.visuals().iter().filter(|v| v.model == "OIL").count();
    println!("oil blobs: {oil}");
    assert!((1..=3).contains(&oil));
}

#[test]
#[ignore]
fn trace_projectiles() {
    let mut s = scene(1200.0);
    println!("car0 {:?} car1 {:?}", s.cars[0].pos, s.cars[1].pos);
    for w in [Weapon::Shockwave, Weapon::Firework] {
        s.grant(0, w);
        for k in 0..12 {
            s.run(0.05, [DriveInput::default(); 2], [k == 0, false]);
            println!("   car1 pos {:?} vel {:?} angvel {:?}", s.cars[1].pos.round(), s.cars[1].vel.round(), s.cars[1].ang_vel);
            let v: Vec<String> = s.weapons.visuals().iter().filter(|v| v.model != "PICKUP" && v.model != "STAR").map(|v| format!("{} {:?}", v.model, v.pos.round())).collect();
            println!("{w:?} t={:.2} {:?}", (k + 1) as f32 * 0.05, v);
        }
    }
}
