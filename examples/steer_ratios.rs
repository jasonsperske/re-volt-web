//! Lists each car's per-wheel SteerRatio (debugging aid).
#[path = "../tests/common/mod.rs"]
mod common;
fn main() {
    let game = common::open_game().expect("disc");
    for e in pollster::block_on(game.list_cars()).unwrap() {
        let car = pollster::block_on(game.load_car(&e.id)).unwrap();
        let r: Vec<String> = car.params.wheels.iter().map(|w| format!("{:+.2}{}", w.steer_ratio, if w.turnable { "T" } else { " " })).collect();
        println!("{:10} {}", e.id, r.join(" "));
    }
}
