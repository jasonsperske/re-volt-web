//! `.fob` level objects (pickup spawns, animated props, ...). Layout
//! verified on every level: `i32 count`, then 56-byte records.

use super::Reader;

#[derive(Clone, Debug)]
pub struct LevelObject {
    /// Object kind; see docs/weapons.md for the known values.
    pub kind: i32,
    pub flags: [i32; 4],
    pub pos: [f32; 3],
    pub up: [f32; 3],
    pub look: [f32; 3],
}

pub fn parse(data: &[u8]) -> Result<Vec<LevelObject>, String> {
    if data.len() < 4 {
        return Ok(Vec::new());
    }
    let mut r = Reader::new(data);
    let count = r.i32()?;
    if count < 0 || count as usize * 56 > r.remaining() {
        return Err(format!("bad object count {count}"));
    }
    (0..count)
        .map(|_| {
            Ok(LevelObject {
                kind: r.i32()?,
                flags: [r.i32()?, r.i32()?, r.i32()?, r.i32()?],
                pos: r.vec3()?,
                up: r.vec3()?,
                look: r.vec3()?,
            })
        })
        .collect()
}
