//! Prints which zone each racing-line node falls in (debugging aid). Usage: zones_along_line LEVEL
#[path = "../tests/common/mod.rs"]
mod common;
use glam::Vec3;
fn main() {
    let id = std::env::args().nth(1).unwrap();
    let game = common::open_game().expect("disc");
    let level = pollster::block_on(game.load_level(&id)).unwrap();
    let track = revolt_web::race::Track::new(level.zones.clone(), level.pos_nodes.clone()).unwrap();
    println!("start node {} total {}", level.pos_nodes.start, level.pos_nodes.total);
    for (i, n) in level.pos_nodes.nodes.iter().enumerate() {
        let p = Vec3::from(n.pos);
        println!("node {i:3} dist {:8.0} next {:?} prev {:?} zone {:?} interp {:8.0}", n.dist, n.next, n.prev, track.zone_at(p), track.dist_to_finish(p));
    }
    for z in &level.zones {
        println!("zone {:2} pos {:?} size {:?} rot0 {:?}", z.id, z.pos.map(|v| v.round()), z.size.map(|v| v.round()), z.rot[0]);
    }
}
