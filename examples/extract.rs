//! Copies one file off the disc image (e.g. for analysing REVOLT.EXE).
//! Usage: extract <DISC/PATH> <out-file>
#[path = "../tests/common/mod.rs"]
mod common;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (src, dst) = (a.get(1).expect("disc path"), a.get(2).expect("output path"));
    let game = common::open_game().expect("disc");
    let data = pollster::block_on(game.disc().read_file(src)).unwrap();
    if let Some(dir) = std::path::Path::new(dst).parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(dst, &data).unwrap();
    println!("{src} -> {dst} ({} bytes)", data.len());
}
