//! `.ncp` collision polygons. Layout confirmed both against every file on the
//! disc and against the game's loader (REVOLT.EXE `sub_46b240`: reads an i16
//! count then 0x70-byte records, counts bit 0 as quad, grows each bbox by 2).

use super::Reader;

pub const NCP_QUAD: i32 = 0x1;

#[derive(Clone, Debug)]
pub struct CollPoly {
    /// Bit 0 = quad. Other bits (0x2, 0x4, 0x20, 0x400, 0x800) are not yet
    /// identified; everything is treated as solid for now.
    pub flags: i32,
    /// Surface material index (grip/sound/skid behaviour in the game).
    pub material: i32,
    /// Surface plane: `dot(normal, p) + d`, positive on the solid's outside.
    pub plane: [f32; 4],
    /// Edge planes; a point on the surface is inside when every edge
    /// distance is <= 0. Only the first 3 are valid for triangles.
    pub edges: [[f32; 4]; 4],
    /// xmin, xmax, ymin, ymax, zmin, zmax
    pub bbox: [f32; 6],
}

impl CollPoly {
    pub fn edge_count(&self) -> usize {
        if self.flags & NCP_QUAD != 0 { 4 } else { 3 }
    }
}

pub fn parse(data: &[u8]) -> Result<Vec<CollPoly>, String> {
    if data.is_empty() {
        return Ok(Vec::new()); // several props ship a zero-byte .ncp
    }
    let mut r = Reader::new(data);
    let count = r.i16()?;
    if count < 0 {
        return Err(format!("bad collision polygon count {count}"));
    }
    let mut polys = Vec::with_capacity(count as usize);
    let plane = |r: &mut Reader| -> Result<[f32; 4], String> { Ok([r.f32()?, r.f32()?, r.f32()?, r.f32()?]) };
    for _ in 0..count {
        let flags = r.i32()?;
        let material = r.i32()?;
        let p = plane(&mut r)?;
        let edges = [plane(&mut r)?, plane(&mut r)?, plane(&mut r)?, plane(&mut r)?];
        let mut bbox = [0.0; 6];
        for b in &mut bbox {
            *b = r.f32()?;
        }
        polys.push(CollPoly { flags, material, plane: p, edges, bbox });
    }
    // Level files follow with a lookup grid (x0, z0, xcount, zcount, cell
    // size, then per-cell index lists). The engine builds its own grid over
    // level and instance polygons together, so it is not read here.
    Ok(polys)
}
