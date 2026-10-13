//! Checks the .fan AI-node layout guess on every level (debugging aid).
#[path = "../tests/common/mod.rs"]
mod common;
fn main() {
    let game = common::open_game().expect("disc");
    for e in pollster::block_on(game.list_levels()).unwrap() {
        let Ok(d) = pollster::block_on(game.read(&format!("LEVELS/{0}/{0}.FAN", e.id))) else {
            println!("{:14} no .fan", e.id);
            continue;
        };
        if d.len() < 4 {
            println!("{:14} {} bytes", e.id, d.len());
            continue;
        }
        let count = u16::from_le_bytes([d[0], d[1]]) as usize;
        let second = u16::from_le_bytes([d[2], d[3]]);
        let tail = d.len() as isize - (4 + 76 * count) as isize;
        let f = |o: usize| f32::from_le_bytes(d[o..o + 4].try_into().unwrap());
        let i = |o: usize| i32::from_le_bytes(d[o..o + 4].try_into().unwrap());
        let mut widths = Vec::new();
        let mut bad_links = 0;
        let mut lines = Vec::new();
        for n in 0..count {
            let o = 4 + 76 * n;
            let l = [f(o + 48), f(o + 52), f(o + 56)];
            let r = [f(o + 64), f(o + 68), f(o + 72)];
            widths.push(((l[0] - r[0]).powi(2) + (l[1] - r[1]).powi(2) + (l[2] - r[2]).powi(2)).sqrt());
            lines.push(f(o + 4));
            for k in 0..4 {
                let link = i(o + 28 + 4 * k);
                if link != -1 && (link < 0 || link as usize >= count) {
                    bad_links += 1;
                }
            }
        }
        widths.sort_by(f32::total_cmp);
        lines.sort_by(f32::total_cmp);
        println!(
            "{:14} {count:3} nodes, u16 {second:3}, tail {tail:3} bytes, width {:.0}..{:.0}, line {:.2}..{:.2}, bad links {bad_links}{}",
            e.id, widths.first().unwrap_or(&0.0), widths.last().unwrap_or(&0.0), lines.first().unwrap_or(&0.0), lines.last().unwrap_or(&0.0),
            if tail > 0 { format!(" tail {:02x?}", &d[d.len() - tail as usize..]) } else { String::new() }
        );
    }
}
