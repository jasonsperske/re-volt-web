//! `.fan` AI nodes: the racing line as a graph of track cross-sections.
//! Layout verified on every level (4 + 76·count + 8 bytes, all links valid).

use super::Reader;

#[derive(Clone, Debug)]
pub struct AiNode {
    pub priority: u8,
    /// Racing line across the track: 0 = left edge, 1 = right edge.
    pub racing_line: f32,
    /// Alternative line for overtaking (same scale).
    pub overtake_line: f32,
    /// Linked node indices, -1 when unused.
    pub prev: [i32; 2],
    pub next: [i32; 2],
    /// Track edges at this cross-section (game space).
    pub left: [f32; 3],
    pub right: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct AiNodes {
    pub nodes: Vec<AiNode>,
    pub start: i32,
    pub total: f32,
}

pub fn parse(data: &[u8]) -> Result<AiNodes, String> {
    let mut r = Reader::new(data);
    let count = r.u16()? as usize;
    let _routes = r.u16()?;
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        let priority = r.u8()?;
        r.skip(3)?;
        let racing_line = r.f32()?;
        let _dist = r.f32()?;
        let overtake_line = r.f32()?;
        r.skip(12)?; // kind, two speed values
        let prev = [r.i32()?, r.i32()?];
        let next = [r.i32()?, r.i32()?];
        r.skip(4)?;
        let left = r.vec3()?;
        r.skip(4)?;
        let right = r.vec3()?;
        nodes.push(AiNode { priority, racing_line, overtake_line, prev, next, left, right });
    }
    let (start, total) = if r.remaining() >= 8 { (r.i32()?, r.f32()?) } else { (0, 0.0) };
    Ok(AiNodes { nodes, start, total })
}
