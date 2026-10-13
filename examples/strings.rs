//! Prints the game's PC strings with their ids (debugging aid).
//! Usage: strings ENGLISH.TXT [text to find]...
fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("ENGLISH.TXT path");
    let s = revolt_web::frontend::parse_strings(&std::fs::read(path).unwrap());
    println!("{} strings", s.len());
    for want in a {
        let hits: Vec<usize> = (0..s.len()).filter(|&i| s[i].eq_ignore_ascii_case(&want)).collect();
        println!("{want:24} {hits:?}");
    }
}
