//! Prints AI nodes (debugging aid). Usage: fan_nodes LEVEL [from] [to]
#[path = "../tests/common/mod.rs"]
mod common;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let game = common::open_game().expect("disc");
    let level = pollster::block_on(game.load_level(&a[1])).unwrap();
    let n = &level.ai_nodes.nodes;
    let from: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let to: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(n.len());
    for i in from..to.min(n.len()) {
        let c = |v: [f32; 3]| [v[0].round(), v[1].round(), v[2].round()];
        println!("{i:3} pri {} line {:.2} prev {:?} next {:?} L {:?} R {:?}", n[i].priority, n[i].racing_line, n[i].prev, n[i].next, c(n[i].left), c(n[i].right));
    }
    let forks: Vec<usize> = (0..n.len()).filter(|&i| n[i].next.iter().filter(|&&j| j >= 0).count() > 1).collect();
    println!("forks at {forks:?}");
    for &f in &forks {
        let pri: Vec<String> = n[f].next.iter().filter(|&&j| j >= 0).map(|&j| format!("{j}(pri {})", n[j as usize].priority)).collect();
        println!("  {f} -> {}", pri.join(", "));
    }
}
