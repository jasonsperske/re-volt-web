//! Counts .fob object kinds on every level (debugging aid).
#[path = "../tests/common/mod.rs"]
mod common;
fn main() {
    let game = common::open_game().expect("disc");
    for e in pollster::block_on(game.list_levels()).unwrap() {
        let level = pollster::block_on(game.load_level(&e.id)).unwrap();
        let mut kinds = std::collections::BTreeMap::new();
        for o in &level.objects {
            *kinds.entry(o.kind).or_insert(0) += 1;
        }
        println!("{:14} {kinds:?}", e.id);
    }
}
