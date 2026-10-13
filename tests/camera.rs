//! The chase camera should be smooth: drive a lap section at 60 fps with
//! frame-time jitter and compare view jitter against the previous camera
//! (raw car pose, absolute-position easing, look straight at the car).

mod common;

use glam::{Quat, Vec3};
use revolt_web::camera::Camera;
use revolt_web::car::{Car, CarSetup, DriveInput};
use revolt_web::collision::level_world;
use revolt_web::scene::to_gl;

/// RMS of the second difference of a per-frame series: how much it jerks
/// rather than how much it moves.
fn jerk(series: &[Vec3]) -> f32 {
    let d2: Vec<f32> = series.windows(3).map(|w| (w[2] - 2.0 * w[1] + w[0]).length_squared()).collect();
    (d2.iter().sum::<f32>() / d2.len() as f32).sqrt()
}

/// RMS frame-to-frame change in apparent velocity (displacement / frame
/// time). Frames of different length move different distances, which is
/// correct; what reads as judder is the velocity jumping.
fn velocity_jerk(series: &[Vec3], dts: &[f32]) -> f32 {
    let v: Vec<Vec3> = series.windows(2).zip(dts).map(|(w, dt)| (w[1] - w[0]) / *dt).collect();
    let d: Vec<f32> = v.windows(2).map(|w| (w[1] - w[0]).length_squared()).collect();
    (d.iter().sum::<f32>() / d.len() as f32).sqrt()
}

#[test]
fn chase_camera_is_smoother_than_before() {
    let Some(game) = common::open_game() else { return };
    let level = pollster::block_on(game.load_level("NHOOD1")).unwrap();
    let assets = pollster::block_on(game.load_car("RC")).unwrap();
    let (lo, hi) = assets.body_bounds();
    let world = level_world(&level);
    let start = Vec3::from(level.info.start_pos);
    let rot = Quat::from_rotation_y(level.info.start_rot * std::f32::consts::TAU);
    let mut car = Car::new(CarSetup::new(&assets.params, assets.hull.as_ref(), lo, hi), start, rot);
    car.place_on_ground(&world, start, rot);

    let mut new_cam = Camera::new(Vec3::ZERO, 0.0, 0.0);
    let (mut old_pos, mut old_dir) = (None::<Vec3>, Vec::new());
    let (mut new_dir, mut new_pos, mut old_pos_series) = (Vec::new(), Vec::new(), Vec::new());
    // Where the drawn car sits relative to the camera: what reads as bounce.
    let (mut old_rel, mut new_rel) = (Vec::new(), Vec::new());
    let mut seed = 7u32;
    let mut dts = Vec::new();
    for frame in 0..400 {
        // Browser frame times wobble around 16.7 ms.
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let dt = 1.0 / 60.0 + ((seed >> 16) as f32 / 65536.0 - 0.5) * 0.004;
        dts.push(dt);
        let steer = (frame as f32 * 0.05).sin() * 0.35;
        car.update(&world, DriveInput { throttle: 1.0, steer }, dt);

        // Previous camera: raw pose, position eased, looks straight at the car.
        let (raw, fwd) = (to_gl(car.pos), to_gl(car.forward()));
        let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
        let target = raw + Vec3::Y * 25.0;
        let desired = target - flat * 230.0 + Vec3::Y * 85.0;
        let p = old_pos.map_or(desired, |p: Vec3| p.lerp(desired, 1.0 - (-8.0 * dt).exp()));
        old_pos = Some(p);
        old_dir.push((target - p).normalize());
        old_pos_series.push(p);
        old_rel.push(raw - p); // drawn at the raw physics pose

        // New camera on the interpolated pose.
        let (pos, rot) = car.render_pose();
        new_cam.chase(to_gl(pos), to_gl(rot * Vec3::Z), 0.0, dt, |_, to| to);
        new_dir.push(new_cam.forward());
        new_pos.push(new_cam.pos);
        new_rel.push(to_gl(pos) - new_cam.pos); // drawn at the interpolated pose
    }
    let skip = 60; // let both settle
    let (od, nd) = (jerk(&old_dir[skip..]), jerk(&new_dir[skip..]));
    let (op, np) = (velocity_jerk(&old_pos_series[skip..], &dts[skip + 1..]), velocity_jerk(&new_pos[skip..], &dts[skip + 1..]));
    let (orl, nrl) = (jerk(&old_rel[skip..]), jerk(&new_rel[skip..]));
    println!("view direction jerk: before {od:.5}, after {nd:.5} ({:.0}% less)", 100.0 * (1.0 - nd / od));
    println!("car-on-screen jerk:  before {orl:.2}, after {nrl:.2} ({:.0}% less)", 100.0 * (1.0 - nrl / orl));
    println!("camera velocity jerk: before {op:.0}, after {np:.0} units/s (includes real acceleration)");
    assert!(nd < od * 0.5, "view direction should jerk at most half as much");
    assert!(nrl < orl * 0.5, "the car should jump around on screen at most half as much");
}
