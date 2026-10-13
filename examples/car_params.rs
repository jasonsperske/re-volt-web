//! Prints parsed handling parameters for a few cars (debugging aid).
#[path = "../tests/common/mod.rs"]
mod common;
fn main() {
    let game = common::open_game().expect("disc");
    for id in std::env::args().skip(1) {
        let car = pollster::block_on(game.load_car(&id)).unwrap();
        let p = &car.params;
        let (lo, hi) = car.body_bounds();
        let s = revolt_web::car::CarSetup::new(p, car.hull.as_ref(), lo, hi);
        println!("{id}: top {} mph engine_rate {} steer_rate {} mass {} inertia {:?} body {:?}..{:?}", p.top_speed, p.engine_rate, p.steer_rate, s.mass, p.body.inertia, lo, hi);
        for (w, sp) in p.wheels.iter().zip(&p.springs) {
            println!("   wheel c {:?} r {} max {} pow {} steer {} sf {} kf {} grip {} | spring k {} d {}", w.center(), w.radius, w.max_pos, w.powered, w.steer_ratio, w.static_friction, w.kinetic_friction, w.grip, sp.stiffness, sp.damping);
        }
        println!("   hull {:?}", s.hull);
    }
}
