//! `.pan` position nodes: a graph along the racing line where each node
//! stores its distance to the finish, used to rank cars. Layout verified on
//! every level (12 + 48·count bytes).

use super::Reader;

#[derive(Clone, Debug)]
pub struct PosNode {
    pub pos: [f32; 3],
    /// Distance from this node to the finish line along the track.
    pub dist: f32,
    /// Linked node indices, -1 for unused slots.
    pub prev: [i32; 4],
    pub next: [i32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct PosNodes {
    pub start: i32,
    /// Total lap length.
    pub total: f32,
    pub nodes: Vec<PosNode>,
}

pub fn parse(data: &[u8]) -> Result<PosNodes, String> {
    let mut r = Reader::new(data);
    let count = r.i32()?;
    let start = r.i32()?;
    let total = r.f32()?;
    if count < 0 || count as usize * 48 > r.remaining() {
        return Err(format!("bad position node count {count}"));
    }
    let links = |r: &mut Reader| -> Result<[i32; 4], String> { Ok([r.i32()?, r.i32()?, r.i32()?, r.i32()?]) };
    let nodes = (0..count)
        .map(|_| Ok(PosNode { pos: r.vec3()?, dist: r.f32()?, prev: links(&mut r)?, next: links(&mut r)? }))
        .collect::<Result<_, String>>()?;
    Ok(PosNodes { start, total, nodes })
}
