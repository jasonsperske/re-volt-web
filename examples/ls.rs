//! Lists disc files under a prefix with sizes (debugging aid). Usage: ls PREFIX
#[path = "../tests/common/mod.rs"]
mod common;
fn main() {
    let prefix = std::env::args().nth(1).unwrap_or_default().to_ascii_uppercase();
    let game = common::open_game().expect("disc");
    for (path, entry) in game.disc().files().filter(|(p, _)| p.starts_with(&prefix)) {
        println!("{:9} {path}", entry.size);
    }
}
