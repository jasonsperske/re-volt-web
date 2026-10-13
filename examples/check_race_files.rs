//! Checks .taz and .pan layouts on every level (debugging aid).
#[path = "../tests/common/mod.rs"]
mod common;
use revolt_web::formats::{pan, taz};
fn main() {
    let game = common::open_game().expect("disc");
    for e in pollster::block_on(game.list_levels()).unwrap() {
        let base = format!("LEVELS/{0}/{0}", e.id);
        let t = pollster::block_on(game.read(&format!("{base}.TAZ"))).ok();
        let p = pollster::block_on(game.read(&format!("{base}.PAN"))).ok();
        let tz = t.as_ref().map(|d| (d.len(), taz::parse(d).map(|z| (z.len(), z.iter().map(|z| z.id).collect::<Vec<_>>()))));
        let pn = p.as_ref().map(|d| (d.len(), pan::parse(d).map(|n| (n.nodes.len(), n.start, n.total, 12 + 48 * n.nodes.len()))));
        println!("{:14} taz {:?}\n{:14} pan {:?}", e.id, tz, "", pn);
    }
}
